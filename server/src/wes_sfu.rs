use core::error::Error;
use std::{collections::HashMap, sync::Arc};

use log::{error, info};
use shared::{RoomID, tcp_command::TcpCommand, tcp_command_id::TcpCommandId};
use tokio::{
    net::{TcpListener, UdpSocket},
    sync::{Mutex, RwLock},
};

use crate::{room::Room, tcp_handler::TcpHandler, udp_handler::UdpHandler};

pub struct WeSFU {
    tcp_listener: TcpListener,
    udp_socket: UdpSocket,
    room_map_for_tcp: Arc<RwLock<HashMap<RoomID, Room>>>,
    room_map_for_udp: Arc<RwLock<HashMap<RoomID, Room>>>,
}

impl WeSFU {
    pub async fn bind(
        tcp_addr: String,
        udp_addr: String,
    ) -> Result<Self, Box<dyn Error + Send + Sync>> {
        let room_map_for_tcp = Arc::new(RwLock::new(HashMap::new()));
        let room_map_for_udp = room_map_for_tcp.clone();

        Ok(Self {
            tcp_listener: TcpListener::bind(tcp_addr).await?,
            udp_socket: UdpSocket::bind(udp_addr).await?,
            room_map_for_tcp,
            room_map_for_udp,
        })
    }

    pub async fn listen(self) -> Result<(), Box<dyn Error + Send + Sync>> {
        let mut udp_task: tokio::task::JoinHandle<Result<(), Box<dyn Error + Send + Sync>>> =
            tokio::spawn(async move {
                let handler = UdpHandler::new();

                handler
                    .handle_socket(self.udp_socket, self.room_map_for_udp)
                    .await?;

                return Ok(());
            });

        let users = Arc::new(RwLock::new(Vec::new()));

        let username_to_tcp_command_tx = Arc::new(Mutex::new(HashMap::new()));

        loop {
            let username_to_tcp_command_tx = username_to_tcp_command_tx.clone();

            let users = users.clone();
            let room_map = self.room_map_for_tcp.clone();

            tokio::select! {

                result = &mut udp_task => {

                    return result?;
                }

                result = self.tcp_listener.accept() => {

                    let tcp_socket = result?.0;

                    tokio::spawn(async move {

                        let users = users.clone();

                        let mut current_username_option = None;
                        let mut current_sid_option = None;

                        if let Err(e) = TcpHandler::handle_stream(tcp_socket, &mut current_username_option, &mut current_sid_option, users.clone(), room_map.clone(), username_to_tcp_command_tx.clone()).await {

                            error!("Error handling TcpSocket: {}", e);
                        }

                        if let Some(current_username) = current_username_option.take() {

                            users.write().await.retain(|user| user != &current_username);
                            username_to_tcp_command_tx
                            .lock()
                            .await
                            .remove(&current_username);

                            if let Some(sid) = current_sid_option {

                                for room in room_map.write().await.values_mut() {

                                    let mut stream_id_to_socket_addr_guard = room.stream_id_to_socket_addr.lock().await;
                                    if stream_id_to_socket_addr_guard.contains_key(&sid) {

                                        stream_id_to_socket_addr_guard.remove(&sid);

                                        room.users.retain(|user| user != &current_username);

                                        for user in room.users.clone() {

                                            if let Some(tx) = username_to_tcp_command_tx.lock().await.get(&user) {

                                                let _ = tx.send(TcpCommand::Bytes(TcpCommandId::OtherUserLeftRoom, sid.to_vec()));
                                            }
                                        }
                                    }
                                }
                            }

                            info!("{} has disconnected", current_username);
                        }

                    });
                }
            }
        }
    }
}
