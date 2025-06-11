use core::error::Error;

use log::info;
use shared::{
    MAX_NAME_LENGTH, is_valid_name, received_tcp_command::ReceivedTcpCommand,
    tcp_command::TcpCommand, tcp_command_id::TcpCommandId,
};
use tokio::net::TcpStream;

use crate::tcp_command_handler::TcpCommandHandler;

pub struct TcpHandler {}

impl TcpHandler {
    pub async fn handle_stream(
        mut stream: TcpStream,
        current_username_option: &mut Option<String>,
    ) -> Result<(), Box<dyn Error>> {
        let current_username = match Self::handle_handshake(&mut stream).await? {
            Some(username) => username,
            None => return Ok(()),
        };

        *current_username_option = Some(current_username.clone());
        info!("{} has connected!", current_username);

        loop {
            let incoming_command = match TcpCommand::read_from_tcp_stream(&mut stream).await? {
                ReceivedTcpCommand::EOF => return Ok(()),
                ReceivedTcpCommand::Command(command) => command,
            };

            TcpCommandHandler::handle_command(&incoming_command).await?;
        }
    }

    async fn handle_handshake(stream: &mut TcpStream) -> Result<Option<String>, Box<dyn Error>> {
        let received_command = match TcpCommand::read_from_tcp_stream(stream).await? {
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
                .write_to_tcp_stream(stream)
                .await?;

            info!("Client sent invalid username");

            return Ok(None);
        }

        if !is_valid_name(&received_username) {
            let error_message =
                "Username must contain only letters, numbers, underscores (_), or hyphens (-).";
            TcpCommand::String(TcpCommandId::ErrorResponse, error_message.to_string())
                .write_to_tcp_stream(stream)
                .await?;

            info!("Client sent invalid username");

            return Ok(None);
        }

        TcpCommand::Simple(TcpCommandId::HelloFromServer)
            .write_to_tcp_stream(stream)
            .await?;

        return Ok(Some(received_username));
    }
}
