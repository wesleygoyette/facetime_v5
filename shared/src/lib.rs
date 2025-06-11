pub mod received_tcp_command;
pub mod tcp_command;
pub mod tcp_command_id;
pub mod tcp_command_payload_type;

pub const TCP_PORT: u16 = 8040;
pub const UDP_PORT: u16 = 8039;

pub const MAX_NAME_LENGTH: usize = 15;

pub fn is_valid_name(name: &str) -> bool {
    name.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}
