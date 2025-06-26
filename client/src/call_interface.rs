use core::error::Error;
use crossterm::{
    cursor::{self, Hide, Show},
    event::{Event, KeyCode, KeyModifiers},
    execute,
    terminal::{
        self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode,
        enable_raw_mode,
    },
};
use std::{io::stdout, time::Duration};
use tokio::{
    net::{TcpStream, UdpSocket},
    sync::watch::{self, Sender},
    time::Instant,
};

use crate::{
    camera::Camera,
    frame::{Frame, combine_frames_with_buffers, detect_true_color},
    renderer::Renderer,
    udp_handler::{udp_listener_loop, udp_send_loop},
};
use crossterm::event::{self};
use shared::StreamID;
use shared::received_tcp_command::ReceivedTcpCommand;
use shared::tcp_command::TcpCommand;
use shared::tcp_command_id::TcpCommandId;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

const SEND_WIDTH: i32 = 96;
const SEND_HEIGHT: i32 = 54;
const MAX_TERMINAL_WIDTH: u16 = 384;
const MAX_TERMINAL_HEIGHT: u16 = 216;
const MAX_COLOR_TERMINAL_WIDTH: u16 = 201;
const MAX_COLOR_TERMINAL_HEIGHT: u16 = 113;
const TARGET_FPS: u64 = 30;
const FRAME_DURATION: Duration = Duration::from_millis(1000 / TARGET_FPS);

pub struct CallInterface;

impl CallInterface {
    pub async fn run(
        full_sid: &[u8],
        tcp_stream: &mut TcpStream,
        udp_stream: UdpSocket,
        camera_index: i32,
        color_enabled: bool,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        println!("Starting camera ASCII feed... Press Ctrl+C to exit");

        let mut stdout = stdout();

        execute!(
            stdout,
            EnterAlternateScreen,
            Hide,
            cursor::MoveTo(0, 0),
            Clear(ClearType::All)
        )?;
        enable_raw_mode()?;
        let _guard = scopeguard::guard((), |_| {
            let _ = disable_raw_mode();
            let _ = execute!(
                stdout,
                LeaveAlternateScreen,
                Show,
                cursor::MoveTo(0, 0),
                Clear(ClearType::All),
                cursor::MoveTo(0, 0)
            );
        });

        let cancel_token = CancellationToken::new();

        let sid_to_frame_map = Arc::new(Mutex::new(HashMap::new()));
        let udp_stream = Arc::new(udp_stream);

        let (camera_frame_channel_tx, camera_frame_channel_rx) = watch::channel(Frame {
            width: 0,
            height: 0,
            data: Arc::new(Vec::new()),
        });

        let mut udp_listener_loop_task = tokio::spawn(udp_listener_loop(
            udp_stream.clone(),
            sid_to_frame_map.clone(),
            cancel_token.clone(),
        ));

        let mut udp_send_loop_task = tokio::spawn(udp_send_loop(
            udp_stream,
            camera_frame_channel_tx.subscribe(),
            full_sid.to_vec(),
            cancel_token.clone(),
        ));

        let mut render_loop_task = tokio::spawn(render_loop(
            camera_frame_channel_rx,
            sid_to_frame_map.clone(),
            color_enabled,
            cancel_token.clone(),
        ));

        let mut camera_loop_task = tokio::spawn(camera_loop(
            camera_frame_channel_tx,
            camera_index,
            cancel_token.clone(),
        ));

        let mut user_input_loop_task = tokio::spawn(user_input_loop(cancel_token.clone()));

        let result = tokio::select! {
            result = &mut user_input_loop_task => result?,
            result = &mut camera_loop_task => result?,
            result = &mut render_loop_task => result?,
            result = &mut udp_listener_loop_task => result?,
            result = &mut udp_send_loop_task => result?,
            result = tcp_loop(tcp_stream, sid_to_frame_map.clone(), cancel_token.clone()) => result
        };

        cancel_token.cancel();

        let cleanup_tasks = [
            user_input_loop_task,
            camera_loop_task,
            render_loop_task,
            udp_listener_loop_task,
            udp_send_loop_task,
        ];

        let cleanup_timeout = Duration::from_millis(500);
        for task in cleanup_tasks {
            if !task.is_finished() {
                let _ = tokio::time::timeout(cleanup_timeout, task).await;
            }
        }

        result
    }
}

async fn camera_loop(
    camera_frame_channel_tx: Sender<Frame>,
    camera_index: i32,
    cancel_token: CancellationToken,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let mut camera = Camera::new(camera_index)?;
    let mut last_frame_time = Instant::now();

    loop {
        tokio::select! {
            _ = cancel_token.cancelled() => break,
            _ = tokio::time::sleep_until(last_frame_time + FRAME_DURATION) => {
                match camera.get_frame().await {
                    Ok(mat) => {
                        match Frame::from_mat(&mat, SEND_WIDTH, SEND_HEIGHT) {
                            Ok(frame) => {
                                if camera_frame_channel_tx.receiver_count() > 0 {
                                    let _ = camera_frame_channel_tx.send(frame);
                                }
                                last_frame_time = Instant::now();
                            }
                            Err(e) => {
                                eprintln!("Frame conversion error: {}", e);
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("Camera error: {}", e);
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                }
            }
        }
    }

    Ok(())
}

async fn render_loop(
    mut camera_frame_channel_rx: watch::Receiver<Frame>,
    sid_to_frame_map: Arc<Mutex<HashMap<StreamID, Option<Frame>>>>,
    color_enabled: bool,
    cancel_token: CancellationToken,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let mut last_content = String::new();
    let mut renderer = Renderer::new();
    let true_color = detect_true_color();

    let mut ascii_buffer = String::with_capacity(50000);
    let mut temp_buffers = Vec::with_capacity(10);
    let mut last_terminal_size = (0, 0);

    loop {
        tokio::select! {
            _ = cancel_token.cancelled() => break,
            result = camera_frame_channel_rx.changed() => {
                if let Err(_) = result {
                    break;
                }

                if let Ok(terminal_size) = terminal::size() {

                    let constrained_terminal_size = match color_enabled {
                        true => (terminal_size.0.min(MAX_COLOR_TERMINAL_WIDTH), terminal_size.1.min(MAX_COLOR_TERMINAL_HEIGHT)),
                        false => (terminal_size.0.min(MAX_TERMINAL_WIDTH), terminal_size.1.min(MAX_TERMINAL_HEIGHT))
                    };

                    let size_changed = terminal_size != last_terminal_size;
                    last_terminal_size = terminal_size;

                    let frame = camera_frame_channel_rx.borrow().clone();
                    let mut frames = Vec::with_capacity(10);
                    frames.push(frame);

                    {
                        let frame_map = sid_to_frame_map.lock().await;
                        for frame_option in frame_map.values() {
                            if let Some(frame) = frame_option {
                                frames.push(frame.clone());
                            }
                        }
                    }

                    combine_frames_with_buffers(
                        &frames,
                        constrained_terminal_size.0,
                        constrained_terminal_size.1,
                        terminal_size.0,
                        terminal_size.1,
                        color_enabled,
                        true_color,
                        &mut ascii_buffer,
                        &mut temp_buffers,
                    );

                    if ascii_buffer != last_content || size_changed {
                        if let Err(e) = renderer.update_terminal(&ascii_buffer, terminal_size.0, terminal_size.1, color_enabled) {
                            eprintln!("Render error: {}", e);
                        }
                        std::mem::swap(&mut last_content, &mut ascii_buffer);
                    }
                }
            }
        }
    }

    Ok(())
}

async fn tcp_loop(
    tcp_stream: &mut TcpStream,
    sid_to_frame_string_map: Arc<Mutex<HashMap<StreamID, Option<Frame>>>>,
    cancel_token: CancellationToken,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    loop {
        tokio::select! {
            result = TcpCommand::read_from_stream(tcp_stream) => {
                match result {
                    Ok(ReceivedTcpCommand::EOF) => {
                        return Err("Server closed connection.".into());
                    }
                    Ok(ReceivedTcpCommand::Command(command)) => {
                        match command {
                            TcpCommand::Bytes(TcpCommandId::OtherUserJoinedRoom, sid_bytes) => {
                                if let Ok(sid) = sid_bytes[..].try_into() {
                                    let mut map = sid_to_frame_string_map.lock().await;
                                    map.insert(sid, None);
                                }
                            }
                            TcpCommand::Bytes(TcpCommandId::OtherUserLeftRoom, sid_bytes) => {
                                if let Ok(sid) = <[u8; 4]>::try_from(&sid_bytes[..]) {
                                    let mut map = sid_to_frame_string_map.lock().await;
                                    map.remove(&sid);
                                }
                            }
                            _ => {}
                        }
                    }
                    Err(e) => {
                        eprintln!("TCP error: {}", e);
                        return Err(e);
                    }
                }
            },
            _ = cancel_token.cancelled() => break,
        }
    }

    Ok(())
}

async fn user_input_loop(
    cancel_token: CancellationToken,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let mut interval = tokio::time::interval(Duration::from_millis(16));

    loop {
        tokio::select! {
            _ = cancel_token.cancelled() => break,
            _ = interval.tick() => {
                if event::poll(Duration::from_millis(0)).unwrap_or(false) {
                    match event::read() {
                        Ok(Event::Key(key_event)) => {
                            if key_event.code == KeyCode::Char('c')
                                && key_event.modifiers.contains(KeyModifiers::CONTROL)
                            {
                                break;
                            }
                        }
                        Ok(Event::Resize(_, _)) => {
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    Ok(())
}
