use core::error::Error;
use std::{collections::HashMap, sync::Arc};

use log::info;
use shared::{RoomID, StreamID};
use tokio::{net::UdpSocket, sync::RwLock, task::JoinSet};

use crate::room::Room;

pub struct UdpHandler;

impl UdpHandler {
    pub async fn handle_socket(
        socket: UdpSocket,
        room_map: Arc<RwLock<HashMap<RoomID, Room>>>,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        let socket = Arc::new(socket);
        let mut tasks = JoinSet::new();

        let mut buf = [0u8; 1500];
        let mut payload = Vec::with_capacity(1500);

        let rid_len = RoomID::default().len();
        let sid_len = StreamID::default().len();
        let min_packet_size = rid_len + sid_len + 1;

        let mut to_addrs = Vec::with_capacity(64);

        let info_logging_enabled = log::log_enabled!(log::Level::Info);

        loop {
            let (n, from_addr) = socket.recv_from(&mut buf).await?;

            if n < min_packet_size {
                continue;
            }

            let rid: RoomID = unsafe { buf.get_unchecked(..rid_len).try_into()? };
            let sid: StreamID =
                unsafe { buf.get_unchecked(rid_len..rid_len + sid_len).try_into()? };
            let frame = unsafe { buf.get_unchecked(rid_len + sid_len..n) };

            to_addrs.clear();

            let needs_update = {
                let room_map_read = room_map.read().await;
                if let Some(room) = room_map_read.get(&rid) {
                    let stream_map = room.stream_id_to_socket_addr.lock().await;

                    for (to_sid, to_addr_option) in stream_map.iter() {
                        if to_sid != &sid {
                            if let Some(to_addr) = to_addr_option {
                                to_addrs.push(*to_addr);
                            }
                        }
                    }

                    stream_map.get(&sid).map_or(true, |entry| entry.is_none())
                } else {
                    continue;
                }
            };

            if needs_update {
                let mut room_map_write = room_map.write().await;
                if let Some(room) = room_map_write.get_mut(&rid) {
                    let mut stream_map = room.stream_id_to_socket_addr.lock().await;
                    if let Some(entry) = stream_map.get_mut(&sid) {
                        if entry.is_none() {
                            *entry = Some(from_addr);
                        }
                    }
                }
            }

            if to_addrs.is_empty() {
                continue;
            }

            payload.clear();
            payload.extend_from_slice(unsafe { buf.get_unchecked(rid_len..rid_len + sid_len) });
            payload.extend_from_slice(frame);

            if info_logging_enabled {
                info!(
                    "Received {} bytes from {}, forwarding to {} destinations",
                    n,
                    from_addr,
                    to_addrs.len()
                );
            }

            if to_addrs.len() <= 4 {
                for &to_addr in &to_addrs {
                    if let Err(e) = socket.send_to(&payload, to_addr).await {
                        log::warn!("Failed to send to {}: {}", to_addr, e);
                    }
                }
            } else {
                let payload_clone = payload.clone();
                let socket_clone = Arc::clone(&socket);
                let to_addrs_clone = to_addrs.clone();

                tasks.spawn(async move {
                    let mut send_tasks = JoinSet::new();

                    for to_addr in to_addrs_clone {
                        let payload_ref = payload_clone.clone();
                        let socket_ref = Arc::clone(&socket_clone);

                        send_tasks.spawn(async move {
                            if let Err(e) = socket_ref.send_to(&payload_ref, to_addr).await {
                                log::warn!("Failed to send to {}: {}", to_addr, e);
                            }
                        });
                    }

                    while send_tasks.join_next().await.is_some() {}
                });

                while tasks.len() > 100 {
                    tasks.join_next().await;
                }
            }
        }
    }
}
