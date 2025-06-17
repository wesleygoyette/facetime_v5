use crate::ascii_converter::Frame;
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

async fn render_frames_to_string(
    frames: &[&Frame],
    ascii_converter: Arc<Mutex<AsciiConverter>>,
    width: u16,
    height: u16,
) -> String {
    if frames.is_empty() {
        return String::new();
    }

    let count = frames.len();
    let (frame_width, frame_height, _layout_rows) = if count == 1 {
        return ascii_converter
            .lock()
            .await
            .nibbles_to_ascii(frames[0], width, height)
            .to_string();
    } else if count == 2 {
        if (width as f64) * 0.38 < height as f64 {
            let mut conv = ascii_converter.lock().await;
            let top = conv
                .nibbles_to_ascii(frames[0], width, (height - 1) / 2)
                .to_string();
            let bottom = conv
                .nibbles_to_ascii(frames[1], width, (height - 1) / 2)
                .to_string();
            return format!("{}\n{}", top, bottom);
        } else {
            let mut conv = ascii_converter.lock().await;
            let left = conv
                .nibbles_to_ascii(frames[0], (width - 1) / 2, height - 1)
                .to_string();
            let right = conv
                .nibbles_to_ascii(frames[1], (width - 1) / 2, height - 1)
                .to_string();
            return frames_side_by_side_to_string(&left, &right);
        }
    } else {
        let rows = ((count + 1) / 2) as u16;
        ((width - 2) / 2, (height - rows + 1) / rows, rows)
    };

    let mut result = String::with_capacity(8192);
    let mut ascii_frames = Vec::with_capacity(count);

    {
        let mut conv = ascii_converter.lock().await;
        for &frame in frames {
            ascii_frames.push(
                conv.nibbles_to_ascii(frame, frame_width, frame_height)
                    .to_string(),
            );
        }
    }

    for (i, pair) in ascii_frames.chunks(2).enumerate() {
        if i > 0 {
            result.push_str("\n\n");
        }
        match pair {
            [left, right] => result.push_str(&frames_side_by_side_to_string(left, right)),
            [only] => result.push_str(only),
            _ => {}
        }
    }

    result
}

pub fn frames_side_by_side_to_string(frame1: &str, frame2: &str) -> String {
    let frame1_lines: Vec<&str> = frame1.lines().collect();
    let frame2_lines: Vec<&str> = frame2.lines().collect();
    let max_lines = frame1_lines.len().max(frame2_lines.len());

    let mut result = String::with_capacity(frame1.len() + frame2.len() + max_lines * 3);

    for i in 0..max_lines {
        if i > 0 {
            result.push('\n');
        }

        let line1 = frame1_lines.get(i).copied().unwrap_or("");
        let line2 = frame2_lines.get(i).copied().unwrap_or("");
        result.push_str(line1);
        result.push_str("  ");
        result.push_str(line2);
    }
    result
}
