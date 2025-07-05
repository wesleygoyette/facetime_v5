use crate::jitter_buffer::JitterBuffer;
use core::error::Error;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{BufferSize, StreamConfig};
use std::sync::{Arc, Mutex};
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

pub struct AudioStreamer;

// Simple linear interpolation resampler
struct SimpleResampler {
    input_rate: u32,
    output_rate: u32,
    input_buffer: Vec<f32>,
    phase: f64,
}

impl SimpleResampler {
    fn new(input_rate: u32, output_rate: u32) -> Self {
        Self {
            input_rate,
            output_rate,
            input_buffer: Vec::new(),
            phase: 0.0,
        }
    }

    fn resample(&mut self, input: &[f32]) -> Vec<f32> {
        // Add new samples to the input buffer
        self.input_buffer.extend_from_slice(input);

        let mut output = Vec::new();
        let ratio = self.input_rate as f64 / self.output_rate as f64;

        while self.phase < (self.input_buffer.len() - 1) as f64 {
            let index = self.phase as usize;
            let fraction = self.phase - index as f64;

            // Linear interpolation
            let sample = if index + 1 < self.input_buffer.len() {
                let a = self.input_buffer[index];
                let b = self.input_buffer[index + 1];
                a + (b - a) * fraction as f32
            } else {
                self.input_buffer[index]
            };

            output.push(sample);
            self.phase += ratio;
        }

        // Remove processed samples from buffer, keeping some for interpolation
        if self.phase >= self.input_buffer.len() as f64 {
            self.input_buffer.clear();
            self.phase = 0.0;
        } else {
            let samples_to_remove = self.phase as usize;
            if samples_to_remove > 0 {
                self.input_buffer.drain(..samples_to_remove);
                self.phase -= samples_to_remove as f64;
            }
        }

        output
    }
}

impl AudioStreamer {
    pub async fn stream(
        server_udp_addr: String, // Changed to owned String
        fsid: Vec<u8>,
        cancel_token: CancellationToken, // Added cancellation token
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        // Spawn the audio task on a blocking thread pool to avoid Send issues
        let handle = tokio::task::spawn_blocking(move || {
            Self::run_audio_streams(server_udp_addr, fsid, cancel_token)
        });

        // Wait for the task to complete
        handle.await??;
        Ok(())
    }

    fn run_audio_streams(
        server_udp_addr: String,
        fsid: Vec<u8>,
        cancel_token: CancellationToken,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        // Create a new async runtime for the blocking task
        let rt = tokio::runtime::Runtime::new()?;

        rt.block_on(async { Self::stream_internal(server_udp_addr, fsid, cancel_token).await })
    }

    async fn stream_internal(
        server_udp_addr: String,
        fsid: Vec<u8>,
        cancel_token: CancellationToken,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        // Create UDP socket for sending audio data
        let udp_socket = UdpSocket::bind("0.0.0.0:0").await?;
        udp_socket.connect(&server_udp_addr).await?;
        let udp_socket = Arc::new(udp_socket);

        let host = cpal::default_host();

        let input_device = host.default_input_device().ok_or_else(|| {
            anyhow::anyhow!("No default input device found. Please check your microphone setup.")
        })?;

        let output_device = host.default_output_device().ok_or_else(|| {
            anyhow::anyhow!("No default output device found. Please check your speaker setup.")
        })?;

        let mut supported_input_configs = input_device.supported_input_configs()?;
        let input_config_range = supported_input_configs
            .next()
            .ok_or_else(|| anyhow::anyhow!("No supported input configs found for the device."))?
            .with_max_sample_rate();

        let original_sample_rate = input_config_range.sample_rate();
        let channels = input_config_range.channels();
        let config = StreamConfig {
            channels,
            sample_rate: original_sample_rate,
            buffer_size: BufferSize::Fixed(256),
        };

        // Target sample rate is 8000 Hz, mono output
        let target_sample_rate = 8000u32;
        let target_channels = 1u16;

        // Create jitter buffer with 50ms target latency at 8kHz
        let jitter_buffer = Arc::new(Mutex::new(JitterBuffer::new(target_sample_rate, 50)));

        // Sequence number for packet ordering
        let sequence_number = Arc::new(Mutex::new(0u32));
        let sequence_number_clone = sequence_number.clone();

        // Create resampler (single channel since we're converting to mono)
        let resampler = Arc::new(Mutex::new(SimpleResampler::new(
            original_sample_rate.0,
            target_sample_rate,
        )));

        // Create channel for sending audio data from callback to async task
        let (audio_tx, mut audio_rx) = mpsc::unbounded_channel::<Vec<u8>>();

        // Input stream callback - convert to mono, resample, and send audio data via channel
        let input_data_fn = {
            let seq_num = sequence_number_clone;
            let resampler = resampler.clone();
            let fsid = fsid.clone();

            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                let mut seq = seq_num.lock().unwrap();
                *seq = seq.wrapping_add(1);
                let current_seq = *seq;
                drop(seq);

                // Convert to mono first
                let mono_data = if channels == 1 {
                    // Already mono
                    data.to_vec()
                } else {
                    // Convert multi-channel to mono by averaging
                    let mut mono = Vec::with_capacity(data.len() / channels as usize);
                    for chunk in data.chunks_exact(channels as usize) {
                        let avg = chunk.iter().sum::<f32>() / channels as f32;
                        mono.push(avg);
                    }
                    mono
                };

                // Resample the mono audio data
                let resampled_data = {
                    let mut resampler_lock = resampler.lock().unwrap();
                    resampler_lock.resample(&mono_data)
                };

                // Only send if we have resampled data
                if !resampled_data.is_empty() {
                    // Create packet with sequence number header
                    let mut packet = Vec::with_capacity(12 + resampled_data.len() * 4);
                    packet.extend_from_slice(&fsid);
                    packet.extend_from_slice(&current_seq.to_ne_bytes());

                    // Add resampled audio data
                    for &sample in &resampled_data {
                        packet.extend_from_slice(&sample.to_ne_bytes());
                    }

                    // Send packet via channel (non-blocking)
                    if let Err(_) = audio_tx.send(packet) {}
                }
            }
        };

        let input_err_fn = |_| {};

        let input_stream =
            input_device.build_input_stream(&config, input_data_fn, input_err_fn, None)?;
        input_stream.play()?;

        // Spawn task to send audio packets via UDP
        let send_task = {
            let send_socket = udp_socket.clone();
            let cancel_token = cancel_token.clone();

            tokio::spawn(async move {
                loop {
                    tokio::select! {
                        _ = cancel_token.cancelled() => break,
                        packet = audio_rx.recv() => {
                            match packet {
                                Some(data) => {
                                    let _ = send_socket.send(&data).await;
                                }
                                None => break,
                            }
                        }
                    }
                }
            })
        };

        // Spawn task to receive audio data and put it in the jitter buffer
        let recv_task = {
            let recv_socket = udp_socket.clone();
            let jitter_buffer = jitter_buffer.clone();
            let cancel_token = cancel_token.clone();

            tokio::spawn(async move {
                let mut buf = vec![0u8; 4096]; // Buffer for multiple f32 samples

                loop {
                    tokio::select! {
                        _ = cancel_token.cancelled() => break,
                        result = recv_socket.recv(&mut buf) => {
                            match result {
                                Ok(size) => {
                                    if size < 12 {
                                        continue;
                                    }

                                    let audio_data = &buf[12..size];
                                    if audio_data.len() % 4 != 0 {
                                        continue;
                                    }

                                    // Convert bytes back to f32 samples
                                    let mut samples = Vec::with_capacity(audio_data.len() / 4);
                                    for chunk in audio_data.chunks_exact(4) {
                                        let sample_bytes: [u8; 4] = chunk.try_into().unwrap();
                                        let sample = f32::from_ne_bytes(sample_bytes);
                                        samples.push(sample);
                                    }

                                    // Add to jitter buffer
                                    {
                                        let mut buffer_lock = jitter_buffer.lock().unwrap();
                                        buffer_lock.add_samples(&samples);
                                    }
                                }
                                Err(_) => {
                                    break;
                                }
                            }
                        }
                    }
                }
            })
        };

        let output_config = StreamConfig {
            channels: target_channels,
            sample_rate: cpal::SampleRate(target_sample_rate),
            buffer_size: BufferSize::Fixed(256),
        };

        let output_data_fn = {
            let jitter_buffer = jitter_buffer.clone();

            move |output: &mut [f32], _: &cpal::OutputCallbackInfo| {
                let mut buffer = jitter_buffer.lock().unwrap();
                buffer.adaptive_adjustment();

                for sample in output.iter_mut() {
                    *sample = buffer.get_sample(); // Already f32
                }
            }
        };

        let output_err_fn = |_| {};

        let output_stream = output_device.build_output_stream(
            &output_config,
            output_data_fn,
            output_err_fn,
            None,
        )?;
        output_stream.play()?;

        // Keep the streams alive until cancelled
        tokio::select! {
            _ = cancel_token.cancelled() => {},
            _ = send_task => {},
            _ = recv_task => {},
        }

        Ok(())
    }
}
