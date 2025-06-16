use core::error::Error;
use std::{collections::HashMap, sync::Arc, vec};

use log::{error, info, warn};
use rand::fill;
use shared::{
    MAX_NAME_LENGTH, RoomID, StreamID, is_valid_name, tcp_command::TcpCommand,
    tcp_command_id::TcpCommandId,
};
use tokio::{
    net::TcpStream,
    sync::{Mutex, RwLock, broadcast},
};

use crate::room::Room;

pub struct TcpCommandHandler;

impl TcpCommandHandler {
    pub async fn handle_command(
        incoming_command: &TcpCommand,
        stream: &mut TcpStream,
        current_username: &str,
        current_sid_option: &mut Option<StreamID>,
        users: Arc<RwLock<Vec<String>>>,
        room_map: Arc<RwLock<HashMap<RoomID, Room>>>,
        username_to_tcp_command_tx: Arc<Mutex<HashMap<String, broadcast::Sender<TcpCommand>>>>,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        let result = match incoming_command {
            TcpCommand::Simple(TcpCommandId::GetUserList) => {
                Self::handle_get_user_list(stream, users).await
            }
            TcpCommand::Simple(TcpCommandId::GetRoomList) => {
                Self::handle_get_room_list(stream, room_map).await
            }
            TcpCommand::String(TcpCommandId::CreateRoom, room_name) => {
                Self::handle_create_room(stream, room_map, room_name).await
            }
            TcpCommand::String(TcpCommandId::DeleteRoom, room_name) => {
                Self::handle_delete_room(stream, room_map, room_name).await
            }
            TcpCommand::String(TcpCommandId::JoinRoom, room_name) => {
                Self::handle_join_room(
                    stream,
                    current_username,
                    current_sid_option,
                    room_map,
                    room_name,
                    username_to_tcp_command_tx,
                )
                .await
            }
            TcpCommand::Simple(TcpCommandId::LeaveRoom) => {
                Self::handle_leave_room(
                    current_username,
                    current_sid_option,
                    room_map,
                    username_to_tcp_command_tx,
                )
                .await
            }
            _ => {
                warn!("Unhandled command received: {:?}", incoming_command);
                Self::send_error_response(
                    stream,
                    &format!("Command not supported: {:?}", incoming_command),
                )
                .await
            }
        };

        if let Err(ref e) = result {
            error!("Error handling command {:?}: {}", incoming_command, e);
        }

        result
    }

    async fn handle_get_user_list(
        stream: &mut TcpStream,
        users: Arc<RwLock<Vec<String>>>,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        let users_snapshot = {
            let guard = users.read().await;
            guard.clone()
        };

        info!("Sending user list with {} users", users_snapshot.len());

        TcpCommand::StringList(TcpCommandId::UserList, users_snapshot)
            .write_to_stream(stream)
            .await
            .map_err(|e| format!("Failed to send user list: {}", e).into())
    }

    async fn handle_get_room_list(
        stream: &mut TcpStream,
        room_map: Arc<RwLock<HashMap<RoomID, Room>>>,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        let room_names = {
            let guard = room_map.read().await;
            guard
                .values()
                .map(|room| room.name.clone())
                .collect::<Vec<_>>()
        };

        info!("Sending room list with {} rooms", room_names.len());

        TcpCommand::StringList(TcpCommandId::RoomList, room_names)
            .write_to_stream(stream)
            .await
            .map_err(|e| format!("Failed to send room list: {}", e).into())
    }

    async fn handle_create_room(
        stream: &mut TcpStream,
        room_map: Arc<RwLock<HashMap<RoomID, Room>>>,
        room_name: &str,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        if room_name.trim().is_empty() {
            info!("Client attempted to create room with empty name");
            return Self::send_error_response(stream, "Room name cannot be empty").await;
        }

        if room_name.len() > MAX_NAME_LENGTH {
            info!("Client attempted to create room with name too long");
            return Self::send_error_response(
                stream,
                &format!(
                    "Room name must be less than or equal to {} characters.",
                    MAX_NAME_LENGTH
                ),
            )
            .await;
        }

        if !is_valid_name(room_name) {
            info!("Client attempted to create room with an invalid name");
            return Self::send_error_response(
                stream,
                "Room name must contain only letters, numbers, underscores (_), or hyphens (-).",
            )
            .await;
        }

        let insert_result = {
            let mut room_map_guard = room_map.write().await;
            if room_map_guard.values().any(|room| room.name == room_name) {
                Err(format!("Room '{}' already exists", room_name))
            } else {
                let mut room_id = RoomID::default();
                fill(&mut room_id);
                let new_room = Room::new(room_name);
                room_map_guard.insert(room_id, new_room);
                Ok(room_id)
            }
        };

        match insert_result {
            Ok(room_id) => {
                info!(
                    "Successfully created room '{}' with ID {:?}",
                    room_name, room_id
                );

                TcpCommand::Simple(TcpCommandId::CreateRoomSuccess)
                    .write_to_stream(stream)
                    .await
                    .map_err(|e| {
                        format!("Failed to send create room success response: {}", e).into()
                    })
            }
            Err(msg) => {
                info!("Client attempted to create duplicate room: {}", room_name);
                Self::send_error_response(stream, &msg).await
            }
        }
    }

    async fn handle_delete_room(
        stream: &mut TcpStream,
        room_map: Arc<RwLock<HashMap<RoomID, Room>>>,
        room_name: &str,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        if room_name.trim().is_empty() {
            info!("Client attempted to delete room with empty name");
            return Self::send_error_response(stream, "Room name cannot be empty").await;
        }

        let room_id_result = {
            let mut room_map_guard = room_map.write().await;

            let room_entry = room_map_guard
                .iter()
                .find(|(_, room)| room.name == room_name)
                .map(|(id, room)| (*id, room.users.len()));

            match room_entry {
                Some((room_id, user_count)) => {
                    if user_count > 0 {
                        Err(format!(
                            "Room '{}' cannot be deleted because it still has {} active user(s).",
                            room_name, user_count
                        ))
                    } else {
                        room_map_guard.remove(&room_id);
                        Ok(room_id)
                    }
                }
                None => Err(format!("Room '{}' does not exist", room_name)),
            }
        };

        match room_id_result {
            Ok(room_id) => {
                info!(
                    "Successfully deleted room '{}' with ID {:?}",
                    room_name, room_id
                );

                TcpCommand::Simple(TcpCommandId::DeleteRoomSuccess)
                    .write_to_stream(stream)
                    .await
                    .map_err(|e| {
                        format!("Failed to send delete room success response: {}", e).into()
                    })
            }
            Err(msg) => {
                info!("Client failed to delete room '{}': {}", room_name, msg);
                Self::send_error_response(stream, &msg).await
            }
        }
    }

    async fn handle_join_room(
        stream: &mut TcpStream,
        current_username: &str,
        current_sid_option: &mut Option<StreamID>,
        room_map: Arc<RwLock<HashMap<RoomID, Room>>>,
        room_name: &str,
        username_to_tcp_command_tx: Arc<Mutex<HashMap<String, broadcast::Sender<TcpCommand>>>>,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        if room_name.trim().is_empty() {
            log::info!("Client attempted to join room with empty name");
            return Self::send_error_response(stream, "Room name cannot be empty").await;
        }

        let mut sid = StreamID::default();
        fill(&mut sid);

        let (room_id_opt, other_users, other_sids) = {
            let mut room_map_guard = room_map.write().await;

            if let Some((room_id, room)) = room_map_guard
                .iter_mut()
                .find(|(_, room)| room.name == room_name)
            {
                let mut sid_map = room.stream_id_to_socket_addr.lock().await;
                let other_sids = sid_map.keys().cloned().collect::<Vec<_>>();

                sid_map.insert(sid, None);

                drop(sid_map);

                let other_users = room.users.clone();
                room.users.push(current_username.to_string());

                *current_sid_option = Some(sid);

                (Some(*room_id), other_users, other_sids)
            } else {
                (None, vec![], vec![])
            }
        };

        match room_id_opt {
            Some(rid) => {
                let mut payload = Vec::from(rid);
                payload.extend_from_slice(&sid);

                TcpCommand::Bytes(TcpCommandId::JoinRoomSuccess, payload)
                    .write_to_stream(stream)
                    .await?;

                for user in other_users {
                    let cmd = TcpCommand::Bytes(TcpCommandId::OtherUserJoinedRoom, sid.to_vec());

                    if let Some(tx) = username_to_tcp_command_tx.lock().await.get(&user) {
                        tx.send(cmd)?;
                    }
                }

                for sid in other_sids {
                    TcpCommand::Bytes(TcpCommandId::OtherUserJoinedRoom, sid.to_vec())
                        .write_to_stream(stream)
                        .await?;
                }
            }
            None => {
                log::info!("Client tried to join nonexistent room: '{}'", room_name);
                Self::send_error_response(stream, &format!("Room '{}' does not exist", room_name))
                    .await?;
            }
        }

        Ok(())
    }

    async fn handle_leave_room(
        current_username: &str,
        current_sid_option: &mut Option<StreamID>,
        room_map: Arc<RwLock<HashMap<RoomID, Room>>>,
        username_to_tcp_command_tx: Arc<Mutex<HashMap<String, broadcast::Sender<TcpCommand>>>>,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        let mut leaving_sid: Option<StreamID> = None;
        let mut affected_users: Vec<String> = vec![];

        let current_sid = match current_sid_option {
            Some(sid) => *sid,
            None => return Err("No sid found".into()),
        };

        {
            let mut map_guard = room_map.write().await;

            for room in map_guard.values_mut() {
                if let Some(_) = room
                    .stream_id_to_socket_addr
                    .lock()
                    .await
                    .remove(&current_sid)
                {
                    leaving_sid = Some(current_sid);

                    room.users.retain(|user| user != &current_username);
                    affected_users = room.users.clone();

                    *current_sid_option = None;

                    break;
                }
            }
        }

        if let Some(sid) = leaving_sid {
            let tx_map = username_to_tcp_command_tx.lock().await;
            let cmd = TcpCommand::Bytes(TcpCommandId::OtherUserLeftRoom, sid.to_vec());

            for user in affected_users {
                if let Some(tx) = tx_map.get(&user) {
                    let _ = tx.send(cmd.clone());
                }
            }
        }

        Ok(())
    }

    async fn send_error_response(
        stream: &mut TcpStream,
        error_message: &str,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        TcpCommand::String(TcpCommandId::ErrorResponse, error_message.to_string())
            .write_to_stream(stream)
            .await
            .map_err(|e| format!("Failed to send error response: {}", e).into())
    }
}
