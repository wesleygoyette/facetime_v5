use shared::StreamID;
use std::{collections::HashMap, net::SocketAddr};

#[derive(Clone)]
pub struct Room {
    pub name: String,
    stream_id_to_socket_addr: HashMap<StreamID, SocketAddr>,
}

impl Room {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            stream_id_to_socket_addr: HashMap::new(),
        }
    }
}
