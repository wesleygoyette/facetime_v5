use core::error::Error;

use shared::{
    TCP_PORT, UDP_PORT, received_tcp_command::ReceivedTcpCommand, tcp_command::TcpCommand,
    tcp_command_id::TcpCommandId,
};
use tokio::net::{TcpStream, UdpSocket};

pub struct Client {
    tcp_stream: TcpStream,
    udp_socket_option: Option<UdpSocket>,
    username: String,
}

impl Client {
    pub async fn connect(server_addr: &str, username: &str) -> Result<Self, Box<dyn Error>> {
        let server_tcp_addr = format!("{}:{}", server_addr, TCP_PORT);
        let server_udp_addr = format!("{}:{}", server_addr, UDP_PORT);

        let udp_socket = UdpSocket::bind("0.0.0.0:0").await?;
        udp_socket.connect(&server_udp_addr).await?;

        let mut tcp_stream = TcpStream::connect(server_tcp_addr).await?;

        perform_handshake(&mut tcp_stream, username).await?;

        return Ok(Self {
            tcp_stream,
            username: username.to_string(),
            udp_socket_option: Some(udp_socket),
        });
    }

    pub async fn run(&mut self) -> Result<(), Box<dyn Error>> {
        return Ok(());
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
