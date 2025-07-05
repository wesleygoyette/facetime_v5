use std::collections::VecDeque;
use std::time::{Duration, Instant};

pub struct JitterBuffer {
    buffer: VecDeque<f32>,
    target_size: usize,
    min_size: usize,
    max_size: usize,
    last_adjustment: Instant,
    adjustment_interval: Duration,
    underrun_count: usize,
    overrun_count: usize,
}

impl JitterBuffer {
    pub fn new(sample_rate: u32, target_latency_ms: u32) -> Self {
        let target_size = (sample_rate as f32 * target_latency_ms as f32 / 1000.0) as usize;
        let min_size = target_size / 2;
        let max_size = target_size * 3;

        Self {
            buffer: VecDeque::with_capacity(max_size),
            target_size,
            min_size,
            max_size,
            last_adjustment: Instant::now(),
            adjustment_interval: Duration::from_millis(100),
            underrun_count: 0,
            overrun_count: 0,
        }
    }

    pub fn add_samples(&mut self, samples: &[f32]) {
        // Check for buffer overflow
        if self.buffer.len() + samples.len() > self.max_size {
            self.overrun_count += 1;
            // Remove oldest samples to make room
            let excess = (self.buffer.len() + samples.len()) - self.max_size;
            for _ in 0..excess {
                self.buffer.pop_front();
            }
        }

        // Add new samples
        for &sample in samples {
            self.buffer.push_back(sample);
        }
    }

    pub fn get_sample(&mut self) -> f32 {
        if self.buffer.is_empty() {
            self.underrun_count += 1;
            0.0 // Return silence on underrun
        } else if self.buffer.len() < self.min_size {
            // Buffer is getting low, but not empty
            self.underrun_count += 1;
            self.buffer.pop_front().unwrap_or(0.0)
        } else {
            self.buffer.pop_front().unwrap_or(0.0)
        }
    }

    pub fn adaptive_adjustment(&mut self) {
        let now = Instant::now();
        if now.duration_since(self.last_adjustment) >= self.adjustment_interval {
            let current_size = self.buffer.len();

            // Adjust playback slightly based on buffer level
            if current_size > self.target_size + (self.target_size / 4) {
                // Buffer is getting full, slightly speed up playback by dropping a sample
                self.buffer.pop_front();
            } else if current_size < self.target_size - (self.target_size / 4) {
                // Buffer is getting low, slightly slow down by duplicating a sample
                if let Some(&last_sample) = self.buffer.back() {
                    self.buffer.push_back(last_sample);
                }
            }

            self.last_adjustment = now;
        }
    }
}
