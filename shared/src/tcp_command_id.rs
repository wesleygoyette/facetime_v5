use crate::tcp_command_payload_type::TcpCommandPayloadType;

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

            pub fn from_byte(byte: u8) -> Result<Self, Box<dyn std::error::Error>> {
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
    }
}

impl TcpCommandId {
    pub fn get_payload_type(&self) -> TcpCommandPayloadType {
        match &self {
            TcpCommandId::HelloFromClient => TcpCommandPayloadType::String,
            TcpCommandId::HelloFromServer => TcpCommandPayloadType::Simple,
            TcpCommandId::ErrorResponse => TcpCommandPayloadType::String,
        }
    }
}
