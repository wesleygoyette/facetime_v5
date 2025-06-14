use crate::tcp_command_payload_type::TcpCommandPayloadType;
use core::error::Error;

const COMMAND_BYTE_OFFSET: u8 = 69;

macro_rules! tcp_command_id_enum {
    (
        $(#[$meta:meta])*
        $vis:vis enum $name:ident {
            $($variant:ident),* $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, PartialEq, Eq, Clone, Copy)]
        #[repr(u8)]
        $vis enum $name {
            $($variant),*
        }

        impl $name {

            pub fn to_byte(&self) -> u8 {
                *self as u8 + COMMAND_BYTE_OFFSET
            }

            pub fn from_byte(byte: u8) -> Result<Self, Box<dyn Error + Send + Sync>> {
                match byte.wrapping_sub(COMMAND_BYTE_OFFSET) {
                    $(x if x == $name::$variant as u8 => Ok($name::$variant),)*
                    _ => Err("Invalid TcpCommandId".into()),
                }
            }
        }
    };
}

tcp_command_id_enum! {
    pub enum TcpCommandId {
        HelloFromClient,
        HelloFromServer,
        ErrorResponse,
        GetUserList,
        UserList,
        GetRoomList,
        RoomList,
        CreateRoom,
        CreateRoomSuccess,
        DeleteRoom,
        DeleteRoomSuccess,
        JoinRoom,
        JoinRoomSuccess,
        LeaveRoom,
        OtherUserJoinedRoom,
        OtherUserLeftRoom
    }
}

impl TcpCommandId {
    pub fn get_payload_type(&self) -> TcpCommandPayloadType {
        match &self {
            TcpCommandId::HelloFromServer => TcpCommandPayloadType::Simple,
            TcpCommandId::GetUserList => TcpCommandPayloadType::Simple,
            TcpCommandId::GetRoomList => TcpCommandPayloadType::Simple,
            TcpCommandId::CreateRoomSuccess => TcpCommandPayloadType::Simple,
            TcpCommandId::CreateRoom => TcpCommandPayloadType::String,
            TcpCommandId::DeleteRoomSuccess => TcpCommandPayloadType::Simple,
            TcpCommandId::LeaveRoom => TcpCommandPayloadType::Simple,

            TcpCommandId::HelloFromClient => TcpCommandPayloadType::String,
            TcpCommandId::ErrorResponse => TcpCommandPayloadType::String,
            TcpCommandId::DeleteRoom => TcpCommandPayloadType::String,
            TcpCommandId::JoinRoom => TcpCommandPayloadType::String,

            TcpCommandId::UserList => TcpCommandPayloadType::StringList,
            TcpCommandId::RoomList => TcpCommandPayloadType::StringList,

            TcpCommandId::JoinRoomSuccess => TcpCommandPayloadType::Bytes,
            TcpCommandId::OtherUserJoinedRoom => TcpCommandPayloadType::Bytes,
            TcpCommandId::OtherUserLeftRoom => TcpCommandPayloadType::Bytes,
        }
    }
}
