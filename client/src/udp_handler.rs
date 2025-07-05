use std::{
    collections::{BTreeMap, HashMap, VecDeque},
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

use crate::frame::Frame;

const CHUNK_SIZE: usize = 1350;
const CHUNK_TIMEOUT: Duration = Duration::from_millis(50);
const DELTA_THRESHOLD: f32 = 0.3;
const MIN_BLOCK_SIZE: usize = 64;
const SEQUENCE_WRAP: u32 = 1000000;
const BUFFER_POOL_SIZE: usize = 10;

#[derive(Clone, Debug, PartialEq)]
enum FrameType {
    Full = 0,
    Delta = 1,
    Heartbeat = 2,
}

#[derive(Clone)]
struct DeltaChunk {
    offset: u32,
    data: Vec<u8>,
}

struct FragmentBuffer {
    chunks: BTreeMap<u32, Vec<u8>>,
    last_update: Instant,
    frame_type: FrameType,
    expected_chunks: u32,
    sequence: u32,
}

struct FrameCache {
    last_frame: Option<Vec<u8>>,
    reconstructed_frame: Option<Vec<u8>>,
    last_sequence: u32,
    corrupted: bool,
}

struct BufferPool {
    buffers: VecDeque<Vec<u8>>,
}

impl BufferPool {
    fn new() -> Self {
        let mut buffers = VecDeque::with_capacity(BUFFER_POOL_SIZE);
        for _ in 0..BUFFER_POOL_SIZE {
            buffers.push_back(Vec::with_capacity(1920 * 1080 * 3));
        }
        Self { buffers }
    }

    fn get_buffer(&mut self) -> Vec<u8> {
        self.buffers
            .pop_front()
            .unwrap_or_else(|| Vec::with_capacity(1920 * 1080 * 3))
    }

    fn return_buffer(&mut self, mut buffer: Vec<u8>) {
        if self.buffers.len() < BUFFER_POOL_SIZE {
            buffer.clear();
            self.buffers.push_back(buffer);
        }
    }
}

impl FrameCache {
    fn new() -> Self {
        Self {
            last_frame: None,
            reconstructed_frame: None,
            last_sequence: 0,
            corrupted: false,
        }
    }

    fn mark_corrupted(&mut self) {
        self.corrupted = true;
    }

    fn reset(&mut self, frame: Vec<u8>, sequence: u32) {
        self.last_frame = Some(frame.clone());
        self.reconstructed_frame = Some(frame);
        self.last_sequence = sequence;
        self.corrupted = false;
    }
}

fn create_delta_optimized(old_frame: &[u8], new_frame: &[u8]) -> Option<Vec<DeltaChunk>> {
    if old_frame.len() != new_frame.len() {
        return None;
    }

    let mut deltas = Vec::new();
    let mut total_delta_size = 0;
    let threshold_size = (new_frame.len() as f32 * DELTA_THRESHOLD) as usize;

    let mut i = 0;
    let len = new_frame.len();

    while i < len {
        while i < len && old_frame[i] == new_frame[i] {
            i += 1;
        }

        if i >= len {
            break;
        }

        let start = i;

        while i < len && old_frame[i] != new_frame[i] {
            i += 1;
        }

        let lookahead = std::cmp::min(MIN_BLOCK_SIZE, len - i);
        let mut identical_count = 0;

        while i + identical_count < len
            && identical_count < lookahead
            && old_frame[i + identical_count] == new_frame[i + identical_count]
        {
            identical_count += 1;
        }

        if identical_count < MIN_BLOCK_SIZE / 4 {
            i += identical_count;
            while i < len && old_frame[i] != new_frame[i] {
                i += 1;
            }
        }

        let chunk_size = i - start;
        total_delta_size += chunk_size + 8;

        if total_delta_size >= threshold_size {
            return None;
        }

        deltas.push(DeltaChunk {
            offset: start as u32,
            data: new_frame[start..i].to_vec(),
        });
    }

    Some(deltas)
}

fn serialize_deltas_optimized(deltas: &[DeltaChunk]) -> Vec<u8> {
    let capacity = 4 + deltas.iter().map(|d| 8 + d.data.len()).sum::<usize>();
    let mut result = Vec::with_capacity(capacity);

    result.extend_from_slice(&(deltas.len() as u32).to_be_bytes());

    for delta in deltas {
        result.extend_from_slice(&delta.offset.to_be_bytes());
        result.extend_from_slice(&(delta.data.len() as u32).to_be_bytes());
        result.extend_from_slice(&delta.data);
    }

    result
}

fn deserialize_deltas(data: &[u8]) -> Result<Vec<DeltaChunk>, Box<dyn Error + Send + Sync>> {
    if data.len() < 4 {
        return Err("Invalid delta data".into());
    }

    let count = u32::from_be_bytes(data[0..4].try_into()?) as usize;
    let mut deltas = Vec::with_capacity(count);
    let mut pos = 4;

    for _ in 0..count {
        if pos + 8 > data.len() {
            return Err("Invalid delta format".into());
        }

        let offset = u32::from_be_bytes(data[pos..pos + 4].try_into()?);
        pos += 4;

        let len = u32::from_be_bytes(data[pos..pos + 4].try_into()?) as usize;
        pos += 4;

        if pos + len > data.len() {
            return Err("Invalid delta data length".into());
        }

        deltas.push(DeltaChunk {
            offset,
            data: data[pos..pos + len].to_vec(),
        });

        pos += len;
    }

    Ok(deltas)
}

fn apply_delta_safe(
    base_frame: &mut [u8],
    deltas: &[DeltaChunk],
) -> Result<(), Box<dyn Error + Send + Sync>> {
    for delta in deltas {
        let start = delta.offset as usize;
        let end = start + delta.data.len();
        if end > base_frame.len() {
            return Err(format!(
                "Delta offset {}..{} out of bounds (frame size: {})",
                start,
                end,
                base_frame.len()
            )
            .into());
        }
    }

    for delta in deltas {
        let start = delta.offset as usize;
        let end = start + delta.data.len();
        base_frame[start..end].copy_from_slice(&delta.data);
    }

    Ok(())
}

pub async fn udp_listener_loop(
    udp_stream: Arc<UdpSocket>,
    sid_to_frame_map: Arc<Mutex<HashMap<StreamID, Option<Frame>>>>,
    udp_listener_loop_cancel_token: CancellationToken,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let mut buf = [0; 1500];
    let mut fragment_buffers: HashMap<StreamID, FragmentBuffer> = HashMap::new();
    let mut frame_caches: HashMap<StreamID, FrameCache> = HashMap::new();
    let mut buffer_pool = BufferPool::new();

    loop {
        tokio::select! {
            result = udp_stream.recv(&mut buf) => {
                if let Ok(n) = result {
                    let sid_len = StreamID::default().len();
                    if n > sid_len + 10 {
                        if let Ok(sid) = StreamID::try_from(&buf[..sid_len]) {
                            let frame_type = match buf[sid_len] {
                                0 => FrameType::Full,
                                1 => FrameType::Delta,
                                2 => FrameType::Heartbeat,
                                _ => continue,
                            };

                            let sequence = u32::from_be_bytes(buf[sid_len + 1..sid_len + 5].try_into()?);
                            let chunk_id = u32::from_be_bytes(buf[sid_len + 5..sid_len + 9].try_into()?);
                            let is_last = buf[sid_len + 9] == 1;
                            let chunk_data = &buf[sid_len + 10..n];

                            if frame_type == FrameType::Heartbeat {
                                continue;
                            }

                            let entry = fragment_buffers.entry(sid.clone()).or_insert(FragmentBuffer {
                                chunks: BTreeMap::new(),
                                last_update: Instant::now(),
                                frame_type: frame_type.clone(),
                                expected_chunks: 0,
                                sequence,
                            });

                            if entry.sequence != sequence {
                                entry.chunks.clear();
                                entry.frame_type = frame_type;
                                entry.sequence = sequence;
                            }

                            entry.chunks.insert(chunk_id, chunk_data.to_vec());
                            entry.last_update = Instant::now();

                            if is_last {
                                entry.expected_chunks = chunk_id + 1;

                                if entry.chunks.len() == entry.expected_chunks as usize {
                                    let mut frame_data = buffer_pool.get_buffer();
                                    for chunk in entry.chunks.values() {
                                        frame_data.extend(chunk);
                                    }

                                    let cache = frame_caches.entry(sid.clone()).or_insert_with(FrameCache::new);

                                    let final_frame_data = match entry.frame_type {
                                        FrameType::Full => {
                                            cache.reset(frame_data.clone(), sequence);
                                            Some(frame_data.clone())
                                        },
                                        FrameType::Delta => {
                                            if cache.corrupted {
                                                None
                                            } else if let Some(ref mut base_frame) = cache.reconstructed_frame {
                                                match deserialize_deltas(&frame_data) {
                                                    Ok(deltas) => {
                                                        let mut new_frame = base_frame.clone();
                                                        match apply_delta_safe(&mut new_frame, &deltas) {
                                                            Ok(()) => {
                                                                cache.reconstructed_frame = Some(new_frame.clone());
                                                                cache.last_sequence = sequence;
                                                                Some(new_frame)
                                                            },
                                                            Err(_) => {
                                                                cache.mark_corrupted();
                                                                None
                                                            }
                                                        }
                                                    },
                                                    Err(_) => {
                                                        cache.mark_corrupted();
                                                        None
                                                    }
                                                }
                                            } else {
                                                cache.mark_corrupted();
                                                None
                                            }
                                        },
                                        FrameType::Heartbeat => None,
                                    };

                                    if let Some(final_data) = final_frame_data {
                                        if let Ok(frame) = Frame::from_bytes(&final_data) {
                                            if let Ok(mut guard) = sid_to_frame_map.try_lock() {
                                                guard.insert(sid.clone(), Some(frame));
                                            }
                                        }
                                    }

                                    buffer_pool.return_buffer(frame_data);
                                    fragment_buffers.remove(&sid);
                                }
                            }
                        }
                    }
                }

                fragment_buffers.retain(|sid, fb| {
                    let expired = fb.last_update.elapsed() >= CHUNK_TIMEOUT;
                    if expired {
                        if let Some(cache) = frame_caches.get_mut(sid) {
                            cache.mark_corrupted();
                        }
                    }
                    !expired
                });
            }

            _ = udp_listener_loop_cancel_token.cancelled() => break,
        }
    }

    Ok(())
}

pub async fn udp_send_loop(
    udp_stream: Arc<UdpSocket>,
    mut camera_frame_channel_rx: watch::Receiver<Frame>,
    video_sid: Vec<u8>,
    udp_send_loop_cancel_token: CancellationToken,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let mut last_frame: Option<Vec<u8>> = None;
    let mut sequence: u32 = 0;
    let mut heartbeat_counter = 0;
    let mut packet_buffer = Vec::with_capacity(CHUNK_SIZE + 100);
    const HEARTBEAT_INTERVAL: u32 = 30;

    loop {
        tokio::select! {
            _ = udp_send_loop_cancel_token.cancelled() => break,
            _ = camera_frame_channel_rx.changed() => {
                let frame = camera_frame_channel_rx.borrow().to_bytes();
                sequence = (sequence + 1) % SEQUENCE_WRAP;

                let (frame_type, data_to_send) = if let Some(ref prev_frame) = last_frame {
                    if let Some(deltas) = create_delta_optimized(prev_frame, &frame) {
                        if deltas.is_empty() {
                            heartbeat_counter += 1;
                            if heartbeat_counter >= HEARTBEAT_INTERVAL {
                                heartbeat_counter = 0;
                                (FrameType::Heartbeat, Vec::new())
                            } else {
                                continue;
                            }
                        } else {
                            heartbeat_counter = 0;
                            (FrameType::Delta, serialize_deltas_optimized(&deltas))
                        }
                    } else {
                        heartbeat_counter = 0;
                        (FrameType::Full, frame.clone())
                    }
                } else {
                    heartbeat_counter = 0;
                    (FrameType::Full, frame.clone())
                };

                last_frame = Some(frame);

                if frame_type == FrameType::Heartbeat {
                    packet_buffer.clear();
                    packet_buffer.extend_from_slice(&video_sid);
                    packet_buffer.push(FrameType::Heartbeat as u8);
                    packet_buffer.extend_from_slice(&sequence.to_be_bytes());
                    packet_buffer.extend_from_slice(&0u32.to_be_bytes());
                    packet_buffer.push(1);
                    let _ = udp_stream.send(&packet_buffer).await;
                    continue;
                }

                let chunks: Vec<_> = data_to_send.chunks(CHUNK_SIZE).collect();
                let total_chunks = chunks.len();

                for (i, chunk) in chunks.iter().enumerate() {
                    packet_buffer.clear();
                    packet_buffer.extend_from_slice(&video_sid);
                    packet_buffer.push(frame_type.clone() as u8);
                    packet_buffer.extend_from_slice(&sequence.to_be_bytes());
                    packet_buffer.extend_from_slice(&(i as u32).to_be_bytes());
                    packet_buffer.push((i + 1 == total_chunks) as u8);
                    packet_buffer.extend_from_slice(chunk);

                    let _ = udp_stream.send(&packet_buffer).await;
                }
            }
        }
    }

    Ok(())
}
