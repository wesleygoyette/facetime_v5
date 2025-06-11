use core::error::Error;

use log::{error, info};
use tokio::net::{TcpListener, UdpSocket};

use crate::{tcp_handler::TcpHandler, udp_handler::UdpHandler};

pub struct WeSFU {
    tcp_listener: TcpListener,
    udp_socket: UdpSocket,
}

impl WeSFU {
    pub async fn bind(tcp_addr: String, udp_addr: String) -> Result<Self, Box<dyn Error>> {
        Ok(Self {
            tcp_listener: TcpListener::bind(tcp_addr).await?,
            udp_socket: UdpSocket::bind(udp_addr).await?,
        })
    }

    pub async fn listen(self) -> Result<(), Box<dyn Error + Send + Sync>> {
        let mut udp_task: tokio::task::JoinHandle<Result<(), Box<dyn Error + Send + Sync>>> =
            tokio::spawn(async move {
                UdpHandler::handle_socket(self.udp_socket).await?;

                return Ok(());
            });

        loop {
            tokio::select! {

                result = &mut udp_task => {

                    return result?;
                }

                result = self.tcp_listener.accept() => {

                    let tcp_socket = result?.0;

                    tokio::spawn(async move {

                        let mut current_username_option = None;

                        if let Err(e) = TcpHandler::handle_stream(tcp_socket, &mut current_username_option).await {

                            error!("Error handling TcpSocket: {}", e);
                        }

                        if let Some(current_username) = current_username_option.take() {
                            info!("{} has disconnected", current_username);
                        }
                    });
                }
            }
        }
    }
}
