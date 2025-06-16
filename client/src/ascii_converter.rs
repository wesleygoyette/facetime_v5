use core::error::Error;
use crossterm::{
    ExecutableCommand, QueueableCommand,
    cursor::MoveTo,
    style::Print,
    terminal::{Clear, ClearType},
};
use opencv::{
    core::{AlgorithmHint, Mat, Size},
    imgproc::{COLOR_BGR2GRAY, INTER_LINEAR, cvt_color, resize},
    prelude::*,
};
use std::io::{Write, stdout};

const ASCII_CHARS: &[char] = &[
    ' ', '.', '^', '=', '~', '-', ',', ':', ';', '+', '*', '?', '%', 'S', '#', '@',
];

pub const WIDTH: i32 = 92; //1920 / 3;
pub const HEIGHT: i32 = 28; //1080 / 3;

pub type Frame = Vec<u8>;

pub struct AsciiConverter {
    last_frame: Option<String>,
    terminal_size: Option<(u16, u16)>,
}

impl AsciiConverter {
    pub fn new() -> Self {
        Self {
            last_frame: None,
            terminal_size: None,
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

        let frame = gray;

        let mut resized = Mat::default();
        let size = Size::new(WIDTH, HEIGHT);
        resize(&frame, &mut resized, size, 0.0, 0.0, INTER_LINEAR)?;

        let data = resized.data_bytes()?;
        let mut nibbles = Vec::with_capacity((WIDTH * HEIGHT / 2) as usize);

        for row in 0..HEIGHT {
            let row_start = (row * WIDTH) as usize;

            for col in (0..WIDTH).step_by(2) {
                let x1 = WIDTH - 1 - col;
                let x2 = if col + 1 < WIDTH {
                    WIDTH - 1 - (col + 1)
                } else {
                    0
                };

                let p1 = data[row_start + x1 as usize];
                let nibble1 = ((p1 as u16 * 15) / 255) as u8;

                let nibble2 = if col + 1 < WIDTH {
                    let p2 = data[row_start + x2 as usize];
                    ((p2 as u16 * 15) / 255) as u8
                } else {
                    0
                };

                nibbles.push((nibble1 << 4) | nibble2);
            }
        }

        Ok(nibbles)
    }

    pub fn nibbles_to_ascii(nibbles: &[u8], width: u16, height: u16) -> String {
        let mut grayscale: Vec<u8> = Vec::with_capacity((WIDTH as usize) * (HEIGHT as usize));
        for byte in nibbles {
            let high = (byte >> 4) & 0x0F;
            let low = byte & 0x0F;
            grayscale.push(high);
            grayscale.push(low);
        }

        let mut ascii_art = String::with_capacity((width + 1) as usize * height as usize);
        for y in 0..height {
            let src_y = (y as f32 * (HEIGHT as f32 - 1.0) / (height as f32)).round() as i32;
            let sy = (src_y.max(0).min(HEIGHT - 1)) as usize;

            for x in 0..width {
                let src_x = (x as f32 * (WIDTH as f32) / (width as f32)).round() as i32;
                let sx = (src_x.max(0).min(WIDTH - 1)) as usize;

                let idx = sy * WIDTH as usize + sx;
                let pixel = if idx < grayscale.len() {
                    grayscale[idx]
                } else {
                    0
                };

                let ascii_idx = (pixel as usize * (ASCII_CHARS.len() - 1)) / 15;
                ascii_art.push(ASCII_CHARS[ascii_idx]);
            }

            ascii_art.push('\n');
        }

        ascii_art
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
            Self::clear_terminal()?;
            self.terminal_size = Some((terminal_width, terminal_height));
            self.last_frame = None;
        }

        if size_changed || self.last_frame.is_none() {
            let lines: Vec<&str> = new_content.lines().collect();
            stdout().queue(MoveTo(0, 0))?;

            for (line_num, line) in lines.iter().enumerate() {
                stdout()
                    .queue(MoveTo(0, line_num as u16))?
                    .queue(Print(line))?;
            }

            let last_line_num = lines.len().saturating_sub(1) as u16;
            let last_line_len = lines.last().map(|l| l.len()).unwrap_or(0) as u16;
            stdout()
                .queue(MoveTo(last_line_len, last_line_num))?
                .flush()?;

            self.last_frame = Some(new_content.to_string());
            return Ok(());
        }

        if let Some(ref last) = self.last_frame {
            if self.try_differential_update(last, new_content)? {
                self.last_frame = Some(new_content.to_string());

                let lines: Vec<&str> = new_content.lines().collect();
                let last_line_num = lines.len().saturating_sub(1) as u16;
                let last_line_len = lines.last().map(|l| l.len()).unwrap_or(0) as u16;
                stdout()
                    .queue(MoveTo(last_line_len, last_line_num))?
                    .flush()?;

                return Ok(());
            }
        }

        let lines: Vec<&str> = new_content.lines().collect();
        stdout().queue(MoveTo(0, 0))?;

        for (line_num, line) in lines.iter().enumerate() {
            stdout()
                .queue(MoveTo(0, line_num as u16))?
                .queue(Print(line))?;
        }

        if let Some(ref last) = self.last_frame {
            let new_lines = new_content.lines().count();
            let old_lines = last.lines().count();

            if old_lines > new_lines {
                for line_num in new_lines..old_lines {
                    stdout()
                        .queue(MoveTo(0, line_num as u16))?
                        .queue(Clear(ClearType::CurrentLine))?;
                }
            }
        }

        let lines: Vec<&str> = new_content.lines().collect();
        let last_line_num = lines.len().saturating_sub(1) as u16;
        let last_line_len = lines.last().map(|l| l.len()).unwrap_or(0) as u16;
        stdout()
            .queue(MoveTo(last_line_len, last_line_num))?
            .flush()?;

        self.last_frame = Some(new_content.to_string());
        Ok(())
    }

    fn try_differential_update(
        &self,
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

            let old_chars: Vec<char> = old_line.chars().collect();
            let new_chars: Vec<char> = new_line.chars().collect();
            let max_chars = old_chars.len().max(new_chars.len());

            let mut col = 0;
            while col < max_chars {
                let old_char = old_chars.get(col).copied().unwrap_or(' ');
                let new_char = new_chars.get(col).copied().unwrap_or(' ');

                if old_char != new_char {
                    let start_col = col;
                    let mut end_col = col;

                    while end_col < max_chars {
                        let old_c = old_chars.get(end_col).copied().unwrap_or(' ');
                        let new_c = new_chars.get(end_col).copied().unwrap_or(' ');

                        if old_c == new_c {
                            break;
                        }
                        end_col += 1;
                    }

                    stdout().queue(MoveTo(start_col as u16, line_num as u16))?;

                    if end_col <= new_chars.len() {
                        let changed_str: String = new_chars[start_col..end_col].iter().collect();
                        stdout().queue(Print(changed_str))?;
                    } else {
                        if start_col < new_chars.len() {
                            let changed_str: String = new_chars[start_col..].iter().collect();
                            stdout().queue(Print(changed_str))?;
                        }
                        stdout().queue(Clear(ClearType::UntilNewLine))?;
                    }

                    updated = true;
                    col = end_col;
                } else {
                    col += 1;
                }
            }

            if new_chars.len() < old_chars.len() {
                stdout()
                    .queue(MoveTo(new_chars.len() as u16, line_num as u16))?
                    .queue(Clear(ClearType::UntilNewLine))?;
                updated = true;
            }
        }

        if old_lines.len() > new_lines.len() {
            for line_num in new_lines.len()..old_lines.len() {
                stdout()
                    .queue(MoveTo(0, line_num as u16))?
                    .queue(Clear(ClearType::CurrentLine))?;
            }
            updated = true;
        }

        if updated {
            let lines: Vec<&str> = new_content.lines().collect();
            let last_line_num = lines.len().saturating_sub(1) as u16;
            let last_line_len = lines.last().map(|l| l.len()).unwrap_or(0) as u16;
            stdout()
                .queue(MoveTo(last_line_len, last_line_num))?
                .flush()?;
        }

        Ok(true)
    }

    pub fn clear_terminal() -> Result<(), Box<dyn Error + Send + Sync>> {
        stdout()
            .execute(Clear(ClearType::All))?
            .execute(MoveTo(0, 0))?;
        Ok(())
    }
}
