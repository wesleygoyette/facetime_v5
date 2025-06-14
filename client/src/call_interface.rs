use crate::{frame::Frame, raw_mode_guard::RawModeGuard};
use core::error::Error;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::{
    cursor::{MoveToColumn, MoveToNextLine},
    execute,
    terminal::{Clear, ClearType},
};
use shared::{
    StreamID, received_tcp_command::ReceivedTcpCommand, tcp_command::TcpCommand,
    tcp_command_id::TcpCommandId,
};
use std::collections::HashMap;
use std::hash::Hash;
use std::io::{Write, stdout};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::{
    net::{TcpStream, UdpSocket},
    sync::{broadcast, watch},
};

pub struct CallInterface;

impl CallInterface {
    pub async fn run(
        username: &str,
        room_name: &str,
        full_sid: &[u8],
        tcp_stream: &mut TcpStream,
        udp_stream: UdpSocket,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        println!(
            "Connecting to {} as {} using sid: {:?}...",
            room_name, username, full_sid
        );

        let _raw_mode_guard = RawModeGuard::new()?;

        let sid_to_frame_map_for_tcp = Arc::new(Mutex::new(HashMap::new()));
        let sid_to_frame_map_for_udp = Arc::new(Mutex::new(HashMap::new()));

        let (end_call_tx, end_call_rx) = broadcast::channel::<()>(1);
        let end_call_tx_for_user_input_task = end_call_tx.clone();
        let end_call_tx_for_draw_frames_task = end_call_tx.clone();
        let end_call_rx_for_draw_frames_task = end_call_tx.subscribe();
        let end_call_tx_for_udp_task = end_call_tx.clone();
        let end_call_rx_for_udp_task = end_call_tx.subscribe();
        let end_call_tx_for_camera_task = end_call_tx.clone();

        let (draw_to_screen_tx, draw_to_screen_rx) = watch::channel(Vec::<Frame>::new());

        let user_input_handler_task = tokio::spawn(async move {
            if let Err(e) = wait_for_ctrl_c_press().await {
                eprintln!("Error handling user input: {}", e);
            }

            if let Err(e) = end_call_tx_for_user_input_task.send(()) {
                eprintln!("Error sending to end_call_tx: {}", e);
            };
        });

        let draw_frames_loop_task = tokio::spawn(async move {
            if let Err(e) =
                draw_frames_loop(draw_to_screen_rx, end_call_rx_for_draw_frames_task).await
            {
                eprintln!("Error drawing frame: {}", e);
            }

            let _ = end_call_tx_for_draw_frames_task.send(());
        });
        let udp_handler_task = tokio::spawn(async move {
            if let Err(e) = udp_handler_loop(
                udp_stream,
                sid_to_frame_map_for_tcp,
                end_call_rx_for_udp_task,
            )
            .await
            {
                eprintln!("Error handling udp: {}", e);
            }

            let _ = end_call_tx_for_udp_task.send(());
        });
        let camera_task = tokio::spawn(async move {});

        tcp_handler_loop(tcp_stream, sid_to_frame_map_for_udp, end_call_rx).await?;
        let _ = end_call_tx.send(());

        user_input_handler_task.await?;
        udp_handler_task.await?;
        draw_frames_loop_task.await?;
        camera_task.await?;

        Ok(())
    }
}

async fn udp_handler_loop(
    udp_stream: UdpSocket,
    sid_to_frame_map: Arc<Mutex<HashMap<StreamID, Option<Frame>>>>,
    mut end_call_rx: broadcast::Receiver<()>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let mut buf = [0; 1500];

    loop {
        tokio::select! {

            _ = end_call_rx.recv() => {

                break;
            }

            result = udp_stream.recv(&mut buf) => {

                let n  = result?;

                let sid_len = StreamID::default().len();

                if n < sid_len + 1 {

                    continue;
                }

                let sid = StreamID::try_from(&buf[0..sid_len])?;

                let payload = &buf[sid_len..n];

                dbg!(sid);
                dbg!(payload);

                break;
            }
        }
    }

    Ok(())
}

async fn tcp_handler_loop(
    tcp_stream: &mut TcpStream,
    sid_to_frame_map: Arc<Mutex<HashMap<StreamID, Option<Frame>>>>,
    mut end_call_rx: broadcast::Receiver<()>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    loop {
        tokio::select! {

            _ = end_call_rx.recv() => {

                break;
            }

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
        }
    }

    Ok(())
}

async fn draw_frames_loop(
    mut draw_to_screen_rx: watch::Receiver<Vec<Frame>>,
    mut end_call_rx: broadcast::Receiver<()>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    loop {
        tokio::select! {

            _ = end_call_rx.recv() => {

                break;
            }

            result = draw_to_screen_rx.changed() => {

                result?;
                let _frames = draw_to_screen_rx.borrow_and_update().clone();
            }
        }
    }

    Ok(())
}

async fn wait_for_ctrl_c_press() -> Result<(), Box<dyn Error + Send + Sync>> {
    loop {
        if let Event::Key(KeyEvent {
            code: KeyCode::Char('c'),
            modifiers,
            ..
        }) = event::read()?
        {
            if modifiers.contains(KeyModifiers::CONTROL) {
                return Ok(());
            }
        }
    }
}
