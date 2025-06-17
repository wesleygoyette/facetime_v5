use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
    time::Duration,
};

use core::error::Error;
use shared::StreamID;
use tokio::{
    net::UdpSocket,
    sync::{Mutex, watch},
    time::Instant,
};
use tokio_util::sync::CancellationToken;

use crate::ascii_converter::Frame;

const CHUNK_SIZE: usize = 1200;
const CHUNK_TIMEOUT: Duration = Duration::from_millis(10);

struct FragmentBuffer {
    chunks: BTreeMap<u32, Vec<u8>>,
    last_update: Instant,
}

pub async fn udp_listener_loop(
    udp_stream: Arc<UdpSocket>,
    sid_to_frame_map: Arc<Mutex<HashMap<StreamID, Option<Frame>>>>,
    udp_listener_loop_cancel_token: CancellationToken,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let mut buf = [0; 1500];
    let mut fragment_buffers: HashMap<StreamID, FragmentBuffer> = HashMap::new();

    loop {
        tokio::select! {
            result = udp_stream.recv(&mut buf) => {
                if let Ok(n) = result {
                    let sid_len = StreamID::default().len();
                    if n > sid_len + 5 {
                        if let Ok(sid) = StreamID::try_from(&buf[..sid_len]) {
                            let chunk_id = u32::from_be_bytes(buf[sid_len..sid_len + 4].try_into()?);
                            let is_last = buf[sid_len + 4] == 1;
                            let chunk_data = &buf[sid_len + 5..n];

                            let entry = fragment_buffers.entry(sid.clone()).or_insert(FragmentBuffer {
                                chunks: BTreeMap::new(),
                                last_update: Instant::now(),
                            });

                            entry.chunks.insert(chunk_id, chunk_data.to_vec());
                            entry.last_update = Instant::now();

                            if is_last {
                                let expected_chunks = chunk_id + 1;
                                if entry.chunks.len() == expected_chunks as usize {
                                    let mut frame_data = Vec::with_capacity(expected_chunks as usize * CHUNK_SIZE);
                                    for chunk in entry.chunks.values() {
                                        frame_data.extend(chunk);
                                    }

                                    if let Ok(mut guard) = sid_to_frame_map.try_lock() {
                                        guard.insert(sid.clone(), Some(frame_data));
                                    }

                                    fragment_buffers.remove(&sid);
                                }
                            }
                        }
                    }
                }

                fragment_buffers.retain(|_, fb| fb.last_update.elapsed() < CHUNK_TIMEOUT);
            }

            _ = udp_listener_loop_cancel_token.cancelled() => break,
        }
    }

    Ok(())
}

pub async fn udp_send_loop(
    udp_stream: Arc<UdpSocket>,
    mut camera_frame_channel_rx: watch::Receiver<Frame>,
    full_sid: Vec<u8>,
    udp_send_loop_cancel_token: CancellationToken,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    loop {
        tokio::select! {
            _ = udp_send_loop_cancel_token.cancelled() => break,
            _ = camera_frame_channel_rx.changed() => {
                let frame = camera_frame_channel_rx.borrow().clone();
                let total_chunks = frame.chunks(CHUNK_SIZE).count();

                for (i, chunk) in frame.chunks(CHUNK_SIZE).enumerate() {
                    let mut packet = full_sid.clone();
                    packet.extend_from_slice(&(i as u32).to_be_bytes());
                    packet.push((i + 1 == total_chunks) as u8);
                    packet.extend_from_slice(chunk);
                    let _ = udp_stream.send(&packet).await;
                }
            }
        }
    }

    Ok(())
}
