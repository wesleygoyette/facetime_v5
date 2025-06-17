use crate::ascii_converter::Frame;
use crate::call_renderer::render_frames_to_string;
use crate::udp_handler::{udp_listener_loop, udp_send_loop};
use crate::{ascii_converter::AsciiConverter, camera::Camera, raw_mode_guard::RawModeGuard};
use core::error::Error;
use crossterm::cursor::MoveTo;
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{self, Clear, ClearType};
use shared::StreamID;
use shared::received_tcp_command::ReceivedTcpCommand;
use shared::tcp_command::TcpCommand;
use shared::tcp_command_id::TcpCommandId;
use std::collections::HashMap;
use std::io::{Write, stdout};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::{TcpStream, UdpSocket};
use tokio::sync::{Mutex, watch};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

pub struct CallInterface;

impl CallInterface {
    pub async fn run(
        full_sid: &[u8],
        tcp_stream: &mut TcpStream,
        udp_stream: UdpSocket,
        camera_index: i32,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        println!("Starting camera ASCII feed... Press Ctrl+C to exit");

        let mut camera = Camera::new(camera_index)?;

        let ascii_converter = Arc::new(Mutex::new(AsciiConverter::new()));

        let cancel_token = CancellationToken::new();
        let camera_loop_cancel_token = cancel_token.clone();
        let user_input_loop_cancel_token = cancel_token.clone();
        let tcp_loop_cancel_token = cancel_token.clone();
        let render_loop_cancel_token = cancel_token.clone();
        let udp_listener_loop_cancel_token = cancel_token.clone();
        let udp_send_loop_cancel_token = cancel_token.clone();

        let initial_frame = AsciiConverter::frame_to_nibbles(camera.get_frame().await?)?;
        let (camera_frame_channel_tx, camera_frame_channel_rx) = watch::channel(initial_frame);

        let raw_mode_guard = RawModeGuard::new();
        let sid_to_frame_map = Arc::new(Mutex::new(HashMap::new()));
        let udp_stream = Arc::new(udp_stream);

        let mut udp_listener_loop_task = tokio::spawn(udp_listener_loop(
            udp_stream.clone(),
            sid_to_frame_map.clone(),
            udp_listener_loop_cancel_token,
        ));

        let mut udp_send_loop_task = tokio::spawn(udp_send_loop(
            udp_stream,
            camera_frame_channel_tx.subscribe(),
            full_sid.to_vec(),
            udp_send_loop_cancel_token,
        ));

        let mut render_loop_task = tokio::spawn(render_loop(
            camera_frame_channel_rx,
            sid_to_frame_map.clone(),
            ascii_converter.clone(),
            render_loop_cancel_token,
        ));

        let mut camera_loop_task = tokio::spawn(camera_loop(
            camera,
            camera_frame_channel_tx,
            camera_loop_cancel_token,
        ));

        let mut user_input_loop_task: JoinHandle<Result<(), Box<dyn Error + Send + Sync>>> =
            tokio::spawn(user_input_loop(user_input_loop_cancel_token));

        let result = tokio::select! {
            result = &mut user_input_loop_task => result?,
            result = &mut camera_loop_task => result?,
            result = &mut render_loop_task => result?,
            result = &mut udp_listener_loop_task => result?,
            result = &mut udp_send_loop_task => result?,
            result = tcp_loop(tcp_stream, sid_to_frame_map, tcp_loop_cancel_token) => result
        };

        cancel_token.cancel();

        let cleanup_tasks = vec![
            user_input_loop_task,
            camera_loop_task,
            render_loop_task,
            udp_listener_loop_task,
            udp_send_loop_task,
        ];

        for task in cleanup_tasks {
            if !task.is_finished() {
                let _ = tokio::time::timeout(Duration::from_millis(200), task).await;
            }
        }

        drop(raw_mode_guard);

        let mut stdout = stdout();
        let _ = execute!(stdout, Clear(ClearType::All), MoveTo(0, 0));
        let _ = stdout.flush();

        result
    }
}

async fn camera_loop(
    mut camera: Camera,
    camera_frame_channel_tx: watch::Sender<Frame>,
    camera_loop_cancel_token: CancellationToken,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let mut interval = tokio::time::interval(Duration::from_millis(50));

    loop {
        tokio::select! {
            _ = camera_loop_cancel_token.cancelled() => break,
            _ = interval.tick() => {
                if let Ok(frame_mat) = camera.get_frame().await {
                    if let Ok(frame) = AsciiConverter::frame_to_nibbles(frame_mat) {
                        let _ = camera_frame_channel_tx.send(frame);
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
    ascii_converter: Arc<Mutex<AsciiConverter>>,
    render_loop_cancel_token: CancellationToken,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let mut last_content = String::new();

    loop {
        tokio::select! {
            _ = render_loop_cancel_token.cancelled() => break,
            result = camera_frame_channel_rx.changed() => {

                result?;

                if let Ok((width, height)) = terminal::size() {
                    let frame = camera_frame_channel_rx.borrow().clone();
                    let mut frames = vec![frame];

                    if let Ok(frame_map) = sid_to_frame_map.try_lock() {
                        for frame_option in frame_map.values() {
                            if let Some(frame) = frame_option {
                                frames.push(frame.clone());
                            }
                        }
                    }

                    let borrowed_frames: Vec<&Vec<u8>> = frames.iter().collect();
                    let new_content = render_frames_to_string(&borrowed_frames, ascii_converter.clone(), width, height).await;


                    if new_content != last_content {
                        if let Ok(mut converter) = ascii_converter.try_lock() {
                            let _ = converter.update_terminal_smooth(&new_content, width, height);
                            last_content = new_content;
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

async fn tcp_loop(
    tcp_stream: &mut TcpStream,
    sid_to_frame_map: Arc<Mutex<HashMap<StreamID, Option<Frame>>>>,
    tcp_loop_cancel_token: CancellationToken,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    loop {
        tokio::select! {
            result = TcpCommand::read_from_stream(tcp_stream) => {
                match result? {
                    ReceivedTcpCommand::EOF => {
                        return Err("Server closed connection.".into());
                    }
                    ReceivedTcpCommand::Command(command) => {
                        match command {
                            TcpCommand::Bytes(TcpCommandId::OtherUserJoinedRoom, sid_bytes) => {
                                let sid: Result<StreamID, _> = sid_bytes[..].try_into();
                                if let Ok(sid) = sid {
                                    if let Ok(mut map) = sid_to_frame_map.try_lock() {
                                        map.insert(sid, None);
                                    }
                                }
                            }

                            TcpCommand::Bytes(TcpCommandId::OtherUserLeftRoom, sid_bytes) => {
                                let sid: Result<StreamID, _> = sid_bytes[..].try_into();
                                if let Ok(sid) = sid {
                                    if let Ok(mut map) = sid_to_frame_map.try_lock() {
                                        map.remove(&sid);
                                    }
                                }
                            }

                            _ => {}
                        }
                    }
                }
            }

            _ = tcp_loop_cancel_token.cancelled() => break,
        }
    }

    Ok(())
}

async fn user_input_loop(
    user_input_loop_cancel_token: CancellationToken,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let mut interval = tokio::time::interval(Duration::from_millis(50));

    loop {
        tokio::select! {
            _ = user_input_loop_cancel_token.cancelled() => break,
            _ = interval.tick() => {
                if event::poll(Duration::from_millis(0)).unwrap_or(false) {
                    if let Ok(Event::Key(key_event)) = event::read() {
                        if key_event.code == KeyCode::Char('c')
                            && key_event.modifiers.contains(KeyModifiers::CONTROL)
                        {
                            break;
                        }
                    }
                }
            }
        }
    }

    Ok(())
}
