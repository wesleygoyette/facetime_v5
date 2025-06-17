use core::error::Error;
use crossterm::{
    QueueableCommand,
    cursor::MoveTo,
    style::Print,
    terminal::{Clear, ClearType},
};
use opencv::{
    core::{AlgorithmHint, Mat, Size},
    imgproc::{COLOR_BGR2GRAY, INTER_LINEAR, cvt_color, resize},
    prelude::*,
};
use std::io::{BufWriter, Write, stdout};

const ASCII_CHARS: &[u8] = b" .^=~-,:;+*?%S#@";

pub const WIDTH: i32 = 640;
pub const HEIGHT: i32 = 360;

pub type Frame = Vec<u8>;

pub struct AsciiConverter {
    last_frame: Option<String>,
    terminal_size: Option<(u16, u16)>,
    ascii_buffer: String,
    grayscale_buffer: Vec<u8>,
    writer: BufWriter<std::io::Stdout>,
}

impl AsciiConverter {
    pub fn new() -> Self {
        let capacity = (WIDTH as usize) * (HEIGHT as usize);
        Self {
            last_frame: None,
            terminal_size: None,
            ascii_buffer: String::with_capacity(capacity + HEIGHT as usize),
            grayscale_buffer: Vec::with_capacity(capacity),
            writer: BufWriter::with_capacity(8192, stdout()),
        }
    }

    pub fn frame_to_nibbles(frame: &Mat) -> Result<Frame, Box<dyn Error + Send + Sync>> {
        let mut gray = Mat::default();
        if frame.channels() != 1 {
            cvt_color(
                frame,
                &mut gray,
                COLOR_BGR2GRAY,
                0,
                AlgorithmHint::ALGO_HINT_DEFAULT,
            )?;
        } else {
            gray = frame.clone();
        }

        let mut resized = Mat::default();
        let size = Size::new(WIDTH, HEIGHT);
        resize(&gray, &mut resized, size, 0.0, 0.0, INTER_LINEAR)?;

        let data = resized.data_bytes()?;
        let mut nibbles = Vec::with_capacity((WIDTH * HEIGHT / 2) as usize);

        let width_usize = WIDTH as usize;
        let height_usize = HEIGHT as usize;

        for row in 0..height_usize {
            let row_start = row * width_usize;

            for col in (0..width_usize).step_by(2) {
                let x1 = width_usize - 1 - col;
                let x2 = if col + 1 < width_usize {
                    width_usize - 1 - (col + 1)
                } else {
                    0
                };

                let p1 = data[row_start + x1];
                let nibble1 = ((p1 as u16 * 15) / 255) as u8;

                let nibble2 = if col + 1 < width_usize {
                    let p2 = data[row_start + x2];
                    ((p2 as u16 * 15) / 255) as u8
                } else {
                    0
                };

                nibbles.push((nibble1 << 4) | nibble2);
            }
        }

        Ok(nibbles)
    }

    pub fn nibbles_to_ascii(&mut self, nibbles: &[u8], width: u16, height: u16) -> &str {
        self.grayscale_buffer.clear();
        self.grayscale_buffer
            .reserve(WIDTH as usize * HEIGHT as usize);

        unsafe {
            self.grayscale_buffer.set_len(nibbles.len() * 2);
            let mut i = 0;
            for &byte in nibbles {
                *self.grayscale_buffer.get_unchecked_mut(i) = (byte >> 4) & 0x0F;
                *self.grayscale_buffer.get_unchecked_mut(i + 1) = byte & 0x0F;
                i += 2;
            }
        }

        self.ascii_buffer.clear();

        let width_f = width as f32;
        let height_f = height as f32;
        let width_scale = WIDTH as f32 / width_f;
        let height_scale = (HEIGHT as f32 - 1.0) / height_f;
        let width_usize = WIDTH as usize;
        let ascii_chars_len_minus_1 = ASCII_CHARS.len() - 1;

        for y in 0..height {
            let src_y = (y as f32 * height_scale).round() as usize;
            let sy = src_y.min(HEIGHT as usize - 1);
            let row_offset = sy * width_usize;

            for x in 0..width {
                let src_x = (x as f32 * width_scale).round() as usize;
                let sx = src_x.min(WIDTH as usize - 1);
                let idx = row_offset + sx;

                let pixel = unsafe {
                    *self
                        .grayscale_buffer
                        .get_unchecked(idx.min(self.grayscale_buffer.len() - 1))
                };

                let ascii_idx = (pixel as usize * ascii_chars_len_minus_1) / 15;
                unsafe {
                    self.ascii_buffer
                        .push(*ASCII_CHARS.get_unchecked(ascii_idx) as char);
                }
            }
            self.ascii_buffer.push('\n');
        }

        &self.ascii_buffer
    }

    pub fn update_terminal_smooth(
        &mut self,
        new_content: &str,
        terminal_width: u16,
        terminal_height: u16,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        let size_changed = self
            .terminal_size
            .map(|(w, h)| w != terminal_width || h != terminal_height)
            .unwrap_or(true);

        if size_changed {
            Self::clear_terminal_fast(&mut self.writer)?;
            self.terminal_size = Some((terminal_width, terminal_height));
            self.last_frame = None;
        }

        if size_changed || self.last_frame.is_none() {
            self.render_full_frame(new_content)?;
            self.last_frame = Some(new_content.to_string());
            return Ok(());
        }

        let last_frame = self.last_frame.clone().unwrap();
        if self.try_differential_update_fast(&last_frame, new_content)? {
            self.last_frame = Some(new_content.to_string());
            self.position_cursor_at_end(new_content)?;
            return Ok(());
        }

        self.render_full_frame(new_content)?;
        self.last_frame = Some(new_content.to_string());
        Ok(())
    }

    #[inline]
    fn render_full_frame(&mut self, content: &str) -> Result<(), Box<dyn Error + Send + Sync>> {
        self.writer.queue(MoveTo(0, 0))?;

        for (line_num, line) in content.lines().enumerate() {
            self.writer
                .queue(MoveTo(0, line_num as u16))?
                .queue(Print(line))?;
        }

        self.position_cursor_at_end(content)?;
        Ok(())
    }

    #[inline]
    fn position_cursor_at_end(
        &mut self,
        content: &str,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        let lines: Vec<&str> = content.lines().collect();
        let last_line_num = lines.len().saturating_sub(1) as u16;
        let last_line_len = lines.last().map(|l| l.len()).unwrap_or(0) as u16;
        self.writer
            .queue(MoveTo(last_line_len, last_line_num))?
            .flush()?;
        Ok(())
    }

    fn try_differential_update_fast(
        &mut self,
        old_content: &str,
        new_content: &str,
    ) -> Result<bool, Box<dyn Error + Send + Sync>> {
        let old_lines: Vec<&str> = old_content.lines().collect();
        let new_lines: Vec<&str> = new_content.lines().collect();

        if old_lines.len().abs_diff(new_lines.len()) > 5 {
            return Ok(false);
        }

        let mut updated = false;
        let max_lines = old_lines.len().max(new_lines.len());

        for line_num in 0..max_lines {
            let old_line = old_lines.get(line_num).unwrap_or(&"");
            let new_line = new_lines.get(line_num).unwrap_or(&"");

            if old_line == new_line {
                continue;
            }

            let old_bytes = old_line.as_bytes();
            let new_bytes = new_line.as_bytes();
            let max_len = old_bytes.len().max(new_bytes.len());

            let mut start = 0;
            while start < old_bytes.len().min(new_bytes.len()) {
                if old_bytes[start] != new_bytes[start] {
                    break;
                }
                start += 1;
            }

            if start < max_len {
                self.writer.queue(MoveTo(start as u16, line_num as u16))?;

                if start < new_bytes.len() {
                    let changed_str = &new_line[start..];
                    self.writer.queue(Print(changed_str))?;
                }

                if new_bytes.len() < old_bytes.len() {
                    self.writer.queue(Clear(ClearType::UntilNewLine))?;
                }

                updated = true;
            }
        }

        if old_lines.len() > new_lines.len() {
            for line_num in new_lines.len()..old_lines.len() {
                self.writer
                    .queue(MoveTo(0, line_num as u16))?
                    .queue(Clear(ClearType::CurrentLine))?;
            }
            updated = true;
        }

        if updated {
            self.position_cursor_at_end(new_content)?;
        }

        Ok(true)
    }

    fn clear_terminal_fast(
        writer: &mut BufWriter<std::io::Stdout>,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        writer
            .queue(Clear(ClearType::All))?
            .queue(MoveTo(0, 0))?
            .flush()?;
        Ok(())
    }
}
