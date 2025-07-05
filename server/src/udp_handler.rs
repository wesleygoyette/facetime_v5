use core::error::Error;
use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::Arc,
    time::{Duration, Instant},
};

use shared::{RoomID, StreamID};
use tokio::{
    net::UdpSocket,
    sync::{Mutex, RwLock},
    time::interval,
};

use crate::room::Room;

const BATCH_SIZE: usize = 32;
const BATCH_TIMEOUT: Duration = Duration::from_millis(1);
const RATE_LIMIT_WINDOW: Duration = Duration::from_secs(1);
const MAX_PACKETS_PER_SECOND: usize = 5000;
const BACKPRESSURE_THRESHOLD: usize = 500;

#[derive(Clone)]
struct ClientStats {
    last_seen: Instant,
    packet_count: usize,
    rate_window_start: Instant,
}

struct PacketBatch {
    packets: Vec<(Vec<u8>, Vec<SocketAddr>)>,
    last_flush: Instant,
}

impl PacketBatch {
    fn new() -> Self {
        Self {
            packets: Vec::with_capacity(BATCH_SIZE),
            last_flush: Instant::now(),
        }
    }

    fn should_flush(&self) -> bool {
        self.packets.len() >= BATCH_SIZE
            || (!self.packets.is_empty() && self.last_flush.elapsed() >= BATCH_TIMEOUT)
    }

    fn add_packet(&mut self, payload: Vec<u8>, destinations: Vec<SocketAddr>) {
        self.packets.push((payload, destinations));
    }

    fn clear(&mut self) {
        self.packets.clear();
        self.last_flush = Instant::now();
    }
}

pub struct UdpHandler {
    client_stats: Arc<Mutex<HashMap<SocketAddr, ClientStats>>>,
    packet_batch: Arc<Mutex<PacketBatch>>,
    stats: Arc<Mutex<ServerStats>>,
    socket: Option<Arc<UdpSocket>>,
}

#[derive(Default, Clone)]
struct ServerStats {
    packets_received: u64,
    packets_forwarded: u64,
    packets_dropped: u64,
}

impl UdpHandler {
    pub fn new() -> Self {
        Self {
            client_stats: Arc::new(Mutex::new(HashMap::new())),
            packet_batch: Arc::new(Mutex::new(PacketBatch::new())),
            stats: Arc::new(Mutex::new(ServerStats::default())),
            socket: None,
        }
    }

    pub async fn handle_socket(
        mut self,
        socket: UdpSocket,
        room_map: Arc<RwLock<HashMap<RoomID, Room>>>,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        let socket = Arc::new(socket);
        self.socket = Some(Arc::clone(&socket));

        let cleanup_task = self.spawn_cleanup_task();
        let batch_flush_task = self.spawn_batch_flush_task(Arc::clone(&socket));

        let mut buf = [0u8; 1500];
        let mut to_addrs = Vec::with_capacity(64);

        let rid_len = RoomID::default().len();
        let sid_len = StreamID::default().len();
        let min_packet_size = rid_len + sid_len + 1;

        tokio::select! {
            result = self.run_packet_loop(
                &socket,
                &room_map,
                &mut buf,
                &mut to_addrs,
                rid_len,
                sid_len,
                min_packet_size
            ) => {
                if let Err(e) = result {
                    log::error!("Packet loop error: {}", e);
                }
            }
            _ = cleanup_task => {
                log::info!("Cleanup task completed");
            }
            _ = batch_flush_task => {
                log::info!("Batch flush task completed");
            }
        }

        Ok(())
    }

    async fn run_packet_loop(
        &self,
        socket: &Arc<UdpSocket>,
        room_map: &Arc<RwLock<HashMap<RoomID, Room>>>,
        buf: &mut [u8],
        to_addrs: &mut Vec<SocketAddr>,
        rid_len: usize,
        sid_len: usize,
        min_packet_size: usize,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        loop {
            match socket.recv_from(buf).await {
                Ok((n, from_addr)) => {
                    self.handle_packet(
                        &buf[..n],
                        from_addr,
                        room_map,
                        to_addrs,
                        socket,
                        rid_len,
                        sid_len,
                        min_packet_size,
                    )
                    .await;
                }
                Err(e) => {
                    log::error!("Error receiving UDP packet: {}", e);
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            }
        }
    }

    async fn handle_packet(
        &self,
        buf: &[u8],
        from_addr: SocketAddr,
        room_map: &Arc<RwLock<HashMap<RoomID, Room>>>,
        to_addrs: &mut Vec<SocketAddr>,
        socket: &UdpSocket,
        rid_len: usize,
        sid_len: usize,
        min_packet_size: usize,
    ) {
        {
            let mut stats = self.stats.lock().await;
            stats.packets_received += 1;
        }

        if buf.len() < min_packet_size {
            return;
        }

        if !self.check_rate_limit(from_addr).await {
            let mut stats = self.stats.lock().await;
            stats.packets_dropped += 1;
            return;
        }

        let rid: RoomID = match buf[..rid_len].try_into() {
            Ok(rid) => rid,
            Err(_) => return,
        };

        let sid: StreamID = match buf[rid_len..rid_len + sid_len].try_into() {
            Ok(sid) => sid,
            Err(_) => return,
        };

        to_addrs.clear();

        let (map_type, needs_update) = {
            let room_map_read = room_map.read().await;

            if let Some(room) = room_map_read.get(&rid) {
                let video_map = room.video_stream_id_to_socket_addr.lock().await;
                if video_map.contains_key(&sid) {
                    for (to_sid, to_addr_option) in video_map.iter() {
                        if to_sid != &sid {
                            if let Some(to_addr) = to_addr_option {
                                to_addrs.push(*to_addr);
                            }
                        }
                    }
                    let needs_update = video_map.get(&sid).map_or(true, |entry| entry.is_none());
                    (Some("video"), needs_update)
                } else {
                    drop(video_map);
                    let audio_map = room.audio_stream_id_to_socket_addr.lock().await;
                    if audio_map.contains_key(&sid) {
                        for (to_sid, to_addr_option) in audio_map.iter() {
                            if to_sid != &sid {
                                if let Some(to_addr) = to_addr_option {
                                    to_addrs.push(*to_addr);
                                }
                            }
                        }
                        let needs_update =
                            audio_map.get(&sid).map_or(true, |entry| entry.is_none());
                        (Some("audio"), needs_update)
                    } else {
                        (None, false)
                    }
                }
            } else {
                (None, false)
            }
        };

        if map_type.is_none() {
            return;
        }

        if needs_update {
            let mut room_map_write = room_map.write().await;
            if let Some(room) = room_map_write.get_mut(&rid) {
                match map_type {
                    Some("video") => {
                        let mut video_map = room.video_stream_id_to_socket_addr.lock().await;
                        if let Some(entry) = video_map.get_mut(&sid) {
                            if entry.is_none() {
                                *entry = Some(from_addr);
                            }
                        }
                    }
                    Some("audio") => {
                        let mut audio_map = room.audio_stream_id_to_socket_addr.lock().await;
                        if let Some(entry) = audio_map.get_mut(&sid) {
                            if entry.is_none() {
                                *entry = Some(from_addr);
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        if needs_update {
            let mut room_map_write = room_map.write().await;
            if let Some(room) = room_map_write.get_mut(&rid) {
                let mut stream_map = room.video_stream_id_to_socket_addr.lock().await;
                if let Some(entry) = stream_map.get_mut(&sid) {
                    if entry.is_none() {
                        *entry = Some(from_addr);
                    }
                }
            }
        }

        if to_addrs.is_empty() {
            return;
        }

        let payload = [&buf[rid_len..rid_len + sid_len], &buf[rid_len + sid_len..]].concat();

        {
            let batch = self.packet_batch.lock().await;
            if batch.packets.len() >= BACKPRESSURE_THRESHOLD {
                let mut stats = self.stats.lock().await;
                stats.packets_dropped += 1;
                return;
            }
        }

        if to_addrs.len() <= 3 {
            self.send_immediate(socket, &payload, to_addrs).await;
        } else {
            let mut batch = self.packet_batch.lock().await;
            batch.add_packet(payload, to_addrs.clone());

            if batch.should_flush() {
                drop(batch);
                self.flush_batch().await;
            }
        }
    }

    async fn send_immediate(
        &self,
        socket: &UdpSocket,
        payload: &[u8],
        destinations: &[SocketAddr],
    ) {
        let mut forwarded = 0;
        let mut dropped = 0;

        for &dest in destinations {
            match socket.send_to(payload, dest).await {
                Ok(_) => forwarded += 1,
                Err(_) => dropped += 1,
            }
        }

        if forwarded > 0 || dropped > 0 {
            let mut stats = self.stats.lock().await;
            stats.packets_forwarded += forwarded;
            stats.packets_dropped += dropped;
        }
    }

    async fn flush_batch(&self) {
        let socket = match &self.socket {
            Some(socket) => Arc::clone(socket),
            None => {
                log::error!("Socket not available for batch flush");
                return;
            }
        };

        let packets_to_send = {
            let mut batch = self.packet_batch.lock().await;
            if batch.packets.is_empty() {
                return;
            }

            let packets = std::mem::take(&mut batch.packets);
            batch.clear();
            packets
        };

        let mut by_destination: HashMap<SocketAddr, Vec<Vec<u8>>> = HashMap::new();

        for (payload, destinations) in packets_to_send {
            for dest in destinations {
                by_destination
                    .entry(dest)
                    .or_default()
                    .push(payload.clone());
            }
        }

        let mut forwarded = 0;
        let mut dropped = 0;

        for (dest, payloads) in by_destination {
            for payload in payloads {
                match socket.send_to(&payload, dest).await {
                    Ok(_) => forwarded += 1,
                    Err(e) => {
                        dropped += 1;
                        if dropped % 100 == 0 {
                            log::warn!("Failed to send to {}: {}", dest, e);
                        }
                    }
                }
            }
        }

        if forwarded > 0 || dropped > 0 {
            let mut stats = self.stats.lock().await;
            stats.packets_forwarded += forwarded;
            stats.packets_dropped += dropped;
        }
    }

    async fn check_rate_limit(&self, addr: SocketAddr) -> bool {
        let mut clients = self.client_stats.lock().await;
        let now = Instant::now();

        let client = clients.entry(addr).or_insert_with(|| ClientStats {
            last_seen: now,
            packet_count: 0,
            rate_window_start: now,
        });

        if client.rate_window_start.elapsed() >= RATE_LIMIT_WINDOW {
            client.packet_count = 0;
            client.rate_window_start = now;
        }

        client.packet_count += 1;
        client.last_seen = now;

        client.packet_count <= MAX_PACKETS_PER_SECOND
    }

    fn spawn_cleanup_task(&self) -> tokio::task::JoinHandle<()> {
        let client_stats = Arc::clone(&self.client_stats);

        tokio::spawn(async move {
            let mut cleanup_interval = interval(Duration::from_secs(60));

            loop {
                cleanup_interval.tick().await;

                let mut clients = client_stats.lock().await;
                let before_count = clients.len();

                clients.retain(|_, stats| stats.last_seen.elapsed() < Duration::from_secs(300));

                let removed = before_count - clients.len();
                if removed > 0 {
                    log::info!("Cleaned up {} inactive clients", removed);
                }
            }
        })
    }

    fn spawn_batch_flush_task(&self, socket: Arc<UdpSocket>) -> tokio::task::JoinHandle<()> {
        let packet_batch = Arc::clone(&self.packet_batch);
        let stats = Arc::clone(&self.stats);

        tokio::spawn(async move {
            let mut flush_interval = interval(BATCH_TIMEOUT);

            loop {
                flush_interval.tick().await;

                let mut batch = packet_batch.lock().await;
                if batch.packets.is_empty() || !batch.should_flush() {
                    continue;
                }

                let packets_to_send = std::mem::take(&mut batch.packets);
                batch.clear();
                drop(batch);

                let mut by_destination: HashMap<SocketAddr, Vec<Vec<u8>>> = HashMap::new();

                for (payload, destinations) in packets_to_send {
                    for dest in destinations {
                        by_destination
                            .entry(dest)
                            .or_default()
                            .push(payload.clone());
                    }
                }

                let mut forwarded = 0;
                let mut dropped = 0;

                for (dest, payloads) in by_destination {
                    for payload in payloads {
                        match socket.send_to(&payload, dest).await {
                            Ok(_) => forwarded += 1,
                            Err(e) => {
                                dropped += 1;
                                if dropped % 100 == 0 {
                                    log::warn!("Failed to send to {}: {}", dest, e);
                                }
                            }
                        }
                    }
                }

                if forwarded > 0 || dropped > 0 {
                    let mut stats_guard = stats.lock().await;
                    stats_guard.packets_forwarded += forwarded;
                    stats_guard.packets_dropped += dropped;
                }
            }
        })
    }
}
