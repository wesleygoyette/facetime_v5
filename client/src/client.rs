use core::error::Error;

use shared::{
    TCP_PORT, received_tcp_command::ReceivedTcpCommand, tcp_command::TcpCommand,
    tcp_command_id::TcpCommandId,
};
use tokio::net::TcpStream;

use crate::{
    call_interface::CallInterface, cli_display::CliDisplay, pre_call_interface::PreCallInterface,
};

pub struct Client {}

impl Client {
    pub async fn run(server_addr: &str, username: &str) -> Result<(), Box<dyn Error>> {
        let server_tcp_addr = format!("{}:{}", server_addr, TCP_PORT);

        let mut tcp_stream = TcpStream::connect(server_tcp_addr).await?;

        perform_handshake(&mut tcp_stream, username).await?;
        CliDisplay::print_connected_message(server_addr, username);

        loop {
            let call_info_option = PreCallInterface::run(&mut tcp_stream).await?;

            match call_info_option {
                Some((room_name, full_sid)) => CallInterface::run(&room_name, &full_sid).await?,
                None => return Ok(()),
            };
        }
    }
}

pub async fn perform_handshake(
    tcp_stream: &mut TcpStream,
    username: &str,
) -> Result<(), Box<dyn Error>> {
    TcpCommand::String(TcpCommandId::HelloFromClient, username.to_string())
        .write_to_tcp_stream(tcp_stream)
        .await?;

    let received_command_option = TcpCommand::read_from_tcp_stream(tcp_stream).await?;

    let received_command = match received_command_option {
        ReceivedTcpCommand::EOF => return Err("Unexpected EOF from server during handshake".into()),
        ReceivedTcpCommand::Command(command) => command,
    };

    match received_command {
        TcpCommand::Simple(TcpCommandId::HelloFromServer) => Ok(()),
        TcpCommand::String(TcpCommandId::ErrorResponse, error) => Err(error.into()),
        _ => Err("Invalid command from server during handshake".into()),
    }
}
