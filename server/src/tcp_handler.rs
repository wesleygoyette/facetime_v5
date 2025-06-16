use core::error::Error;
use std::{collections::HashMap, sync::Arc};

use log::info;
use shared::{
    MAX_NAME_LENGTH, RoomID, StreamID, is_valid_name, received_tcp_command::ReceivedTcpCommand,
    tcp_command::TcpCommand, tcp_command_id::TcpCommandId,
};
use tokio::{
    net::TcpStream,
    sync::{Mutex, RwLock, broadcast},
};

use crate::{room::Room, tcp_command_handler::TcpCommandHandler};

pub struct TcpHandler;

impl TcpHandler {
    pub async fn handle_stream(
        mut stream: TcpStream,
        current_username_option: &mut Option<String>,
        current_sid_option: &mut Option<StreamID>,
        users: Arc<RwLock<Vec<String>>>,
        room_map: Arc<RwLock<HashMap<RoomID, Room>>>,
        username_to_tcp_command_tx: Arc<Mutex<HashMap<String, broadcast::Sender<TcpCommand>>>>,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        let current_username = match Self::handle_handshake(&mut stream, users.clone()).await? {
            Some(username) => username,
            None => return Ok(()),
        };

        let current_username = current_username.clone();

        *current_username_option = Some(current_username.clone());
        users.write().await.push(current_username.clone());
        info!("{} has connected!", current_username);

        let (tcp_command_channel_tx, mut tcp_command_channel_rx) = broadcast::channel(16);

        username_to_tcp_command_tx
            .lock()
            .await
            .insert(current_username.clone(), tcp_command_channel_tx);

        loop {
            tokio::select! {

                result = TcpCommand::read_from_stream(&mut stream) => {

                    let incoming_command = match result? {
                        ReceivedTcpCommand::EOF => return Ok(()),
                        ReceivedTcpCommand::Command(command) => command,
                    };

                    TcpCommandHandler::handle_command(
                        &incoming_command,
                        &mut stream,
                        &current_username,
                        current_sid_option,
                        users.clone(),
                        room_map.clone(),
                        username_to_tcp_command_tx.clone(),
                    )
                    .await?;
                }

                result = tcp_command_channel_rx.recv() => {
                    let outgoing_command = result?;

                    outgoing_command.write_to_stream(&mut stream).await?;

                }
            }
        }
    }

    async fn handle_handshake(
        stream: &mut TcpStream,
        users: Arc<RwLock<Vec<String>>>,
    ) -> Result<Option<String>, Box<dyn Error + Send + Sync>> {
        let received_command = match TcpCommand::read_from_stream(stream).await? {
            ReceivedTcpCommand::EOF => return Ok(None),
            ReceivedTcpCommand::Command(cmd) => cmd,
        };

        let received_username = match received_command {
            TcpCommand::String(TcpCommandId::HelloFromClient, username) => username,
            _ => return Err("Invalid hello command from client".into()),
        };

        if received_username.len() > MAX_NAME_LENGTH {
            let error_message = format!(
                "Username must be less than or equal to {} characters.",
                MAX_NAME_LENGTH
            );
            TcpCommand::String(TcpCommandId::ErrorResponse, error_message)
                .write_to_stream(stream)
                .await?;

            info!("Client sent invalid username");

            return Ok(None);
        }

        if !is_valid_name(&received_username) {
            let error_message =
                "Username must contain only letters, numbers, underscores (_), or hyphens (-).";
            TcpCommand::String(TcpCommandId::ErrorResponse, error_message.to_string())
                .write_to_stream(stream)
                .await?;

            info!("Client sent invalid username");

            return Ok(None);
        }

        if users.read().await.contains(&received_username) {
            let error_message = "Username is already taken.";
            TcpCommand::String(TcpCommandId::ErrorResponse, error_message.to_string())
                .write_to_stream(stream)
                .await?;

            info!("Client sent invalid username");

            return Ok(None);
        }

        TcpCommand::Simple(TcpCommandId::HelloFromServer)
            .write_to_stream(stream)
            .await?;

        return Ok(Some(received_username));
    }
}
