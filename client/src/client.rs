use core::error::Error;

use shared::{
    TCP_PORT, UDP_PORT, received_tcp_command::ReceivedTcpCommand, tcp_command::TcpCommand,
    tcp_command_id::TcpCommandId,
};
use tokio::net::{TcpStream, UdpSocket};

use crate::{
    call_interface::CallInterface, cli_display::CliDisplay, pre_call_interface::PreCallInterface,
};

pub struct Client;

impl Client {
    pub async fn run(
        server_addr: &str,
        username: &str,
        camera_index: &mut i32,
        color_enabled: bool,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        let server_tcp_addr = format!("{}:{}", server_addr, TCP_PORT);
        let server_udp_addr = format!("{}:{}", server_addr, UDP_PORT);

        let mut tcp_stream = TcpStream::connect(server_tcp_addr).await?;

        perform_handshake(&mut tcp_stream, username).await?;
        CliDisplay::print_connected_message(server_addr, username);

        let call_info_option =
            PreCallInterface::run(&mut tcp_stream, username, camera_index).await?;

        match call_info_option {
            Some(full_sid) => {
                let udp_stream = UdpSocket::bind("0.0.0.0:0").await?;
                udp_stream.connect(&server_udp_addr).await?;

                if let Err(e) = CallInterface::run(
                    &full_sid,
                    &mut tcp_stream,
                    udp_stream,
                    *camera_index,
                    color_enabled,
                )
                .await
                {
                    eprintln!("Call Error: {}", e);
                }

                TcpCommand::Simple(TcpCommandId::LeaveRoom)
                    .write_to_stream(&mut tcp_stream)
                    .await?;
            }
            None => return Ok(()),
        };

        Ok(())
    }
}

pub async fn perform_handshake(
    tcp_stream: &mut TcpStream,
    username: &str,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    TcpCommand::String(TcpCommandId::HelloFromClient, username.to_string())
        .write_to_stream(tcp_stream)
        .await?;

    let received_command_option = TcpCommand::read_from_stream(tcp_stream).await?;

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
