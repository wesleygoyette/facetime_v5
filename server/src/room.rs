use shared::StreamID;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct Room {
    pub name: String,
    pub stream_id_to_socket_addr: Arc<Mutex<HashMap<StreamID, Option<std::net::SocketAddr>>>>,
    pub users: Vec<String>,
}

impl Room {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            stream_id_to_socket_addr: Arc::new(Mutex::new(HashMap::new())),
            users: vec![],
        }
    }
}
