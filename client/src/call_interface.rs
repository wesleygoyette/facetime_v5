use crate::ascii_converter::Frame;
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
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        println!("Starting camera ASCII feed... Press Ctrl+C to exit");

        let mut camera = Camera::new()?;
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

        let raw_mode_guard = RawModeGuard::new()?;
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

                    let new_content = render_frames_to_string(frames, width, height);

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

async fn udp_listener_loop(
    udp_stream: Arc<UdpSocket>,
    sid_to_frame_map: Arc<Mutex<HashMap<StreamID, Option<Frame>>>>,
    udp_listener_loop_cancel_token: CancellationToken,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let mut buf = [0; 1300];

    loop {
        tokio::select! {
            result = udp_stream.recv(&mut buf) => {
                if let Ok(n) = result {
                    if n > StreamID::default().len() {
                        if let Ok(sid) = <StreamID>::try_from(&buf[..StreamID::default().len()]) {
                            let payload = &buf[StreamID::default().len()..n];
                            let frame_data = payload.to_vec();

                            if let Ok(mut guard) = sid_to_frame_map.try_lock() {
                                if let Some(frame_slot) = guard.get_mut(&sid) {
                                    *frame_slot = Some(frame_data);
                                }
                            }
                        }
                    }
                }
            }

            _ = udp_listener_loop_cancel_token.cancelled() => break,
        }
    }

    Ok(())
}

async fn udp_send_loop(
    udp_stream: Arc<UdpSocket>,
    camera_frame_channel_rx: watch::Receiver<Frame>,
    full_sid: Vec<u8>,
    udp_send_loop_cancel_token: CancellationToken,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let mut interval = tokio::time::interval(Duration::from_millis(30));

    loop {
        tokio::select! {
            _ = udp_send_loop_cancel_token.cancelled() => break,
            _ = interval.tick() => {
                if camera_frame_channel_rx.has_changed().unwrap_or(false) {
                    let frame = camera_frame_channel_rx.borrow().clone();
                    let mut payload = full_sid.clone();
                    payload.extend(frame);
                    let _ = udp_stream.send(&payload).await;
                }
            }
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

fn render_frames_to_string(frames: Vec<Vec<u8>>, width: u16, height: u16) -> String {
    match frames.len() {
        0 => String::new(),
        1 => {
            let my_nibbles = &frames[0];
            AsciiConverter::nibbles_to_ascii(my_nibbles, width, height)
        }
        2 => {
            let my_nibbles = &frames[0];
            let your_nibbles = &frames[1];

            if (width as f64) * 0.38 < (height as f64) {
                let frame1 = AsciiConverter::nibbles_to_ascii(my_nibbles, width, (height - 1) / 2);
                let frame2 =
                    AsciiConverter::nibbles_to_ascii(your_nibbles, width, (height - 1) / 2);
                format!("{}\n{}", frame1, frame2)
            } else {
                let frame1 = AsciiConverter::nibbles_to_ascii(my_nibbles, (width - 1) / 2, height);
                let frame2 =
                    AsciiConverter::nibbles_to_ascii(your_nibbles, (width - 1) / 2, height);
                frames_side_by_side_to_string(&frame1, &frame2)
            }
        }
        len => {
            let num_rows = ((len + 1) / 2) as u16;
            let frame_height = (height - num_rows + 1) / num_rows;
            let frame_width = (width - 2) / 2;

            let ascii_frames: Vec<String> = frames
                .iter()
                .map(|f| AsciiConverter::nibbles_to_ascii(f, frame_width, frame_height))
                .collect();

            let mut result = String::new();
            for (idx, pair) in ascii_frames.chunks(2).enumerate() {
                if idx > 0 {
                    result.push('\n');
                    result.push('\n');
                }
                if pair.len() == 2 {
                    result.push_str(&frames_side_by_side_to_string(&pair[0], &pair[1]));
                } else {
                    result.push_str(&pair[0]);
                }
            }
            result
        }
    }
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
