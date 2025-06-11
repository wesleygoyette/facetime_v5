use core::error::Error;

use log::info;
use tokio::net::UdpSocket;

pub struct UdpHandler {}

impl UdpHandler {
    pub async fn handle_socket(socket: UdpSocket) -> Result<(), Box<dyn Error + Send + Sync>> {
        let mut buf = [0; 1500];

        loop {
            let (n, addr) = socket.recv_from(&mut buf).await?;

            info!("Received {} bytes from {}", n, addr);
        }
    }
}
