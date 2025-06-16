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
        username: &str,
        room_name: &str,
        full_sid: &[u8],
        tcp_stream: &mut TcpStream,
        udp_stream: UdpSocket,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        let mut camera = Camera::new()?;
        let ascii_converter = Arc::new(Mutex::new(AsciiConverter::new()));

        let cancel_token = CancellationToken::new();
        let camera_loop_cancel_token = cancel_token.clone();
        let user_input_loop_cancel_token = cancel_token.clone();
        let tcp_loop_cancel_token = cancel_token.clone();
        let render_loop_cancel_token = cancel_token.clone();

        let (camera_frame_channel_tx, camera_frame_channel_rx) =
            watch::channel(AsciiConverter::frame_to_nibbles(camera.get_frame().await?)?);

        let raw_mode_guard = RawModeGuard::new()?;

        let mut render_loop_task = tokio::spawn(render_loop(
            ascii_converter.clone(),
            camera_frame_channel_rx,
            render_loop_cancel_token,
        ));

        let mut camera_loop_task = tokio::spawn(camera_loop(
            camera,
            camera_frame_channel_tx,
            camera_loop_cancel_token,
        ));

        let mut user_input_loop_task: JoinHandle<Result<(), Box<dyn Error + Send + Sync>>> =
            tokio::spawn(user_input_loop(user_input_loop_cancel_token));

        let sid_to_frame_map = Arc::new(Mutex::new(HashMap::new()));

        let result = tokio::select! {
            result = &mut user_input_loop_task => result?,
            result = &mut camera_loop_task => result?,
            result = &mut render_loop_task => result?,
            result = tcp_loop(tcp_stream, sid_to_frame_map, tcp_loop_cancel_token) => result
        };

        cancel_token.cancel();

        if !user_input_loop_task.is_finished() {
            let _ = tokio::time::timeout(Duration::from_millis(800), user_input_loop_task).await;
        }
        if !camera_loop_task.is_finished() {
            let _ = tokio::time::timeout(Duration::from_millis(800), camera_loop_task).await;
        }
        if !render_loop_task.is_finished() {
            let _ = tokio::time::timeout(Duration::from_millis(800), render_loop_task).await;
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
    loop {
        tokio::select! {
            _ = camera_loop_cancel_token.cancelled() => {
                break;
            }

            result = async {
                let frame_mat = camera.get_frame().await?;

                if !camera_loop_cancel_token.is_cancelled() {

                    let frame = AsciiConverter::frame_to_nibbles(frame_mat)?;
                    camera_frame_channel_tx.send(frame)?;
                }

                Ok::<(), Box<dyn Error + Send + Sync>>(())
            } => {
                if let Err(e) = result {
                    eprintln!("Camera loop error: {}", e);
                    break;
                }
            }
        }
    }

    Ok(())
}
async fn render_loop(
    ascii_converter: Arc<Mutex<AsciiConverter>>,
    mut camera_frame_channel_rx: watch::Receiver<Frame>,
    render_loop_cancel_token: CancellationToken,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    loop {
        if render_loop_cancel_token.is_cancelled() {
            break;
        }

        let frame = camera_frame_channel_rx.borrow_and_update().clone();

        let (width, height) = terminal::size()?;
        let ascii = AsciiConverter::nibbles_to_ascii(&frame, width, height);
        ascii_converter
            .lock()
            .await
            .update_terminal_smooth(&ascii, width, height)?;

        tokio::time::sleep(Duration::from_millis(16)).await;
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
                            TcpCommand::Bytes(TcpCommandId::OtherUserJoinedRoom, sid) => {
                                let sid: StreamID = sid[..].try_into()?;
                                sid_to_frame_map.lock().await.insert(sid, None);
                            }

                            TcpCommand::Bytes(TcpCommandId::OtherUserLeftRoom, sid) => {
                                let sid: StreamID = sid[..].try_into()?;
                                sid_to_frame_map.lock().await.remove(&sid);
                            }

                            _ => {}
                        }
                    }
                }
            }

            _ = tcp_loop_cancel_token.cancelled() => {
                break;
            }
        }
    }

    Ok(())
}

async fn user_input_loop(
    user_input_loop_cancel_token: CancellationToken,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    loop {
        tokio::select! {
            _ = user_input_loop_cancel_token.cancelled() => {
                break;
            }

            result = async {
                if event::poll(Duration::from_millis(50))? {
                    if let Event::Key(key_event) = event::read()? {
                        if key_event.code == KeyCode::Char('c')
                            && key_event.modifiers.contains(KeyModifiers::CONTROL)
                        {
                            return Ok::<bool, Box<dyn Error + Send + Sync>>(true);
                        }
                    }
                }
                Ok::<bool, Box<dyn Error + Send + Sync>>(false)
            } => {
                match result {
                    Ok(true) => break,
                    Ok(false) => {},
                    Err(e) => {
                        eprintln!("Input loop error: {}", e);
                        break;
                    }
                }
            }
        }

        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    Ok(())
}
