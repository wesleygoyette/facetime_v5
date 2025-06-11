use core::error::Error;
use std::{collections::HashMap, sync::Arc};

use log::{error, info, warn};
use rand::fill;
use shared::{
    MAX_NAME_LENGTH, RoomID, is_valid_name, tcp_command::TcpCommand, tcp_command_id::TcpCommandId,
};
use tokio::{net::TcpStream, sync::Mutex};

use crate::room::Room;

pub struct TcpCommandHandler {}

impl TcpCommandHandler {
    pub async fn handle_command(
        incoming_command: &TcpCommand,
        stream: &mut TcpStream,
        users: Arc<Mutex<Vec<String>>>,
        room_map: Arc<Mutex<HashMap<RoomID, Room>>>,
    ) -> Result<(), Box<dyn Error>> {
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
                Self::handle_join_room(stream, room_map, room_name).await
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
        users: Arc<Mutex<Vec<String>>>,
    ) -> Result<(), Box<dyn Error>> {
        let users_snapshot = {
            let guard = users.lock().await;
            guard.clone()
        };

        info!("Sending user list with {} users", users_snapshot.len());

        TcpCommand::StringList(TcpCommandId::UserList, users_snapshot)
            .write_to_tcp_stream(stream)
            .await
            .map_err(|e| format!("Failed to send user list: {}", e).into())
    }

    async fn handle_get_room_list(
        stream: &mut TcpStream,
        room_map: Arc<Mutex<HashMap<RoomID, Room>>>,
    ) -> Result<(), Box<dyn Error>> {
        let room_names = {
            let guard = room_map.lock().await;
            guard
                .values()
                .map(|room| room.name.clone())
                .collect::<Vec<_>>()
        };

        info!("Sending room list with {} rooms", room_names.len());

        TcpCommand::StringList(TcpCommandId::RoomList, room_names)
            .write_to_tcp_stream(stream)
            .await
            .map_err(|e| format!("Failed to send room list: {}", e).into())
    }

    async fn handle_create_room(
        stream: &mut TcpStream,
        room_map: Arc<Mutex<HashMap<RoomID, Room>>>,
        room_name: &str,
    ) -> Result<(), Box<dyn Error>> {
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
            let mut room_map_guard = room_map.lock().await;

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
                    .write_to_tcp_stream(stream)
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
        room_map: Arc<Mutex<HashMap<RoomID, Room>>>,
        room_name: &str,
    ) -> Result<(), Box<dyn Error>> {
        if room_name.trim().is_empty() {
            info!("Client attempted to delete room with empty name");
            return Self::send_error_response(stream, "Room name cannot be empty").await;
        }

        let room_id_result = {
            let mut room_map_guard = room_map.lock().await;

            let room_id_to_delete = room_map_guard
                .iter()
                .find(|(_, room)| room.name == room_name)
                .map(|(id, _)| *id);

            match room_id_to_delete {
                Some(room_id) => {
                    room_map_guard.remove(&room_id);
                    Ok(room_id)
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
                    .write_to_tcp_stream(stream)
                    .await
                    .map_err(|e| {
                        format!("Failed to send delete room success response: {}", e).into()
                    })
            }
            Err(msg) => {
                info!("Client tried to delete nonexistent room: '{}'", room_name);
                Self::send_error_response(stream, &msg).await
            }
        }
    }

    async fn handle_join_room(
        stream: &mut TcpStream,
        room_map: Arc<Mutex<HashMap<RoomID, Room>>>,
        room_name: &str,
    ) -> Result<(), Box<dyn Error>> {
        if room_name.trim().is_empty() {
            info!("Client attempted to join room with empty name");
            return Self::send_error_response(stream, "Room name cannot be empty").await;
        }

        let room_id_result = {
            let room_map_guard = room_map.lock().await;

            room_map_guard
                .iter()
                .find(|(_, room)| room.name == room_name)
                .map(|(id, _)| *id)
                .ok_or_else(|| format!("Room '{}' does not exist", room_name))
        };

        match room_id_result {
            Ok(rid) => {
                let sid = [42];

                let mut payload = Vec::from(rid);
                payload.extend(sid);

                TcpCommand::Bytes(TcpCommandId::JoinRoomSuccess, payload)
                    .write_to_tcp_stream(stream)
                    .await
                    .map_err(|e| format!("Failed to send join room success response: {}", e).into())
            }
            Err(msg) => {
                info!("Client tried to join nonexistent room: '{}'", room_name);
                Self::send_error_response(stream, &msg).await
            }
        }
    }

    async fn send_error_response(
        stream: &mut TcpStream,
        error_message: &str,
    ) -> Result<(), Box<dyn Error>> {
        TcpCommand::String(TcpCommandId::ErrorResponse, error_message.to_string())
            .write_to_tcp_stream(stream)
            .await
            .map_err(|e| format!("Failed to send error response: {}", e).into())
    }
}
