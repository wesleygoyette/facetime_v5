use libwebp_sys::*;
use opencv::{core::AlgorithmHint, prelude::*};
use std::ptr;
use std::sync::Arc;

#[derive(Clone)]
pub struct Frame {
    pub width: i32,
    pub height: i32,
    pub data: Arc<Vec<u8>>,
}

impl Frame {
    pub fn from_mat(mat: &Mat, width: i32, height: i32) -> opencv::Result<Self> {
        use opencv::{
            core::{Mat, Size},
            imgproc::{COLOR_BGR2RGB, INTER_LINEAR, cvt_color, resize},
        };

        let mut rgb = Mat::default();
        cvt_color(
            mat,
            &mut rgb,
            COLOR_BGR2RGB,
            0,
            AlgorithmHint::ALGO_HINT_ACCURATE,
        )?;

        let mut resized = Mat::default();
        resize(
            &rgb,
            &mut resized,
            Size::new(width, height),
            0.0,
            0.0,
            INTER_LINEAR,
        )?;

        let data = Arc::new(resized.data_bytes()?.to_vec());

        assert_eq!(
            data.len(),
            (width * height * 3) as usize,
            "Data length mismatch in from_mat"
        );

        Ok(Self {
            width,
            height,
            data,
        })
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut output_ptr: *mut u8 = ptr::null_mut();

        let output_size = unsafe {
            WebPEncodeRGB(
                self.data.as_ptr(),
                self.width,
                self.height,
                self.width * 3,
                75.0,
                &mut output_ptr,
            )
        };

        if output_size == 0 || output_ptr.is_null() {
            panic!("WebP encoding failed");
        }

        let compressed = unsafe { Vec::from_raw_parts(output_ptr, output_size, output_size) };

        let mut buf = Vec::with_capacity(12 + compressed.len());
        buf.extend(&self.width.to_le_bytes());
        buf.extend(&self.height.to_le_bytes());
        buf.extend(&(compressed.len() as u32).to_le_bytes());
        buf.extend(compressed);

        buf
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        if bytes.len() < 12 {
            return Err("Too short to decode Frame".into());
        }

        let stored_width = i32::from_le_bytes(bytes[0..4].try_into()?);
        let stored_height = i32::from_le_bytes(bytes[4..8].try_into()?);
        let compressed_len = u32::from_le_bytes(bytes[8..12].try_into()?) as usize;

        if bytes.len() < 12 + compressed_len {
            return Err("Not enough bytes for compressed data".into());
        }

        let compressed = &bytes[12..12 + compressed_len];
        let mut out_width = 0;
        let mut out_height = 0;

        let decoded_ptr = unsafe {
            WebPDecodeRGB(
                compressed.as_ptr(),
                compressed_len,
                &mut out_width,
                &mut out_height,
            )
        };

        if decoded_ptr.is_null() {
            return Err("WebP decoding failed".into());
        }

        if out_width != stored_width || out_height != stored_height {
            unsafe { libc::free(decoded_ptr as *mut libc::c_void) };
            return Err("Decoded dimensions do not match stored values".into());
        }

        let pixel_count = out_width * out_height * 3;
        let data =
            unsafe { Vec::from_raw_parts(decoded_ptr, pixel_count as usize, pixel_count as usize) };

        Ok(Self {
            width: out_width,
            height: out_height,
            data: Arc::new(data),
        })
    }

    pub fn to_ascii_with_buffer(
        &self,
        color_enabled: bool,
        true_color: bool,
        width: i32,
        height: i32,
        buffer: &mut String,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        use opencv::{
            core::{Mat, Size},
            imgproc::{INTER_LINEAR, resize},
        };

        const ASCII_CHARS: &[u8] = b" .'`^\",_-|\\/*rxz%@$B";
        const COLOR_ASCII_CHARS: &[u8] = b" `'.,-^~:;!*+=cr?%$S#@";
        const TRUE_COLOR_ASCII_CHARS: &[u8] = b" ,:;lll$$$$&&&&&#####";

        let expected = self.width * self.height * 3;
        if self.data.len() != expected as usize {
            return Err("Frame data size mismatch".into());
        }

        let base = Mat::from_slice(self.data.as_ref())?;
        let mat: opencv::boxed_ref::BoxedRef<'_, Mat> = base.reshape(3, self.height)?;
        let mut resized = Mat::default();
        resize(
            &mat,
            &mut resized,
            Size::new(width, height),
            0.0,
            0.0,
            INTER_LINEAR,
        )?;

        let resized_data = resized.data_bytes()?;

        buffer.clear();
        let capacity = if color_enabled {
            if true_color {
                (width * height * 25 + height) as usize
            } else {
                (width * height * 12 + height) as usize
            }
        } else {
            (width * height + height) as usize
        };
        buffer.reserve(capacity);

        let ascii_chars = if color_enabled && true_color {
            TRUE_COLOR_ASCII_CHARS
        } else if color_enabled {
            COLOR_ASCII_CHARS
        } else {
            ASCII_CHARS
        };
        let ascii_len = ascii_chars.len() - 1;

        for row in 0..height {
            for col in 0..width {
                let idx = (row * width + col) as usize * 3;
                if idx + 2 >= resized_data.len() {
                    continue;
                }

                let r = resized_data[idx];
                let g = resized_data[idx + 1];
                let b = resized_data[idx + 2];

                let gray = ((r as u32 * 77 + g as u32 * 150 + b as u32 * 29) >> 8) as u8;

                let ascii_index = (gray as usize * ascii_len) / 255;
                let c = ascii_chars[ascii_index] as char;

                if color_enabled {
                    if true_color {
                        use std::fmt::Write;
                        let _ = write!(buffer, "\x1b[38;2;{};{};{}m{}", r, g, b, c);
                    } else {
                        let color_code = rgb_to_ansi256_fast(r, g, b);
                        use std::fmt::Write;
                        let _ = write!(buffer, "\x1b[38;5;{}m{}", color_code, c);
                    }
                } else {
                    buffer.push(c);
                }
            }

            if color_enabled {
                buffer.push_str("\x1b[0m\n");
            } else {
                buffer.push('\n');
            }
        }

        Ok(())
    }
}

pub fn combine_frames_with_buffers(
    frames: &[Frame],
    target_width: u16,
    target_height: u16,
    true_width: u16,
    true_height: u16,
    color_enabled: bool,
    true_color: bool,
    ascii_buffer: &mut String,
    temp_buffers: &mut Vec<String>,
) {
    ascii_buffer.clear();

    if frames.is_empty() {
        return;
    }

    let aspect_ratio = frames[0].width as f64 / frames[0].height as f64;
    let count = frames.len();

    let (cols, rows) = match count {
        1 => (1, 1),
        2 => optimal_two_frame_layout(target_width, target_height, aspect_ratio),
        _ => calculate_optimal_grid(count, target_width, target_height, aspect_ratio),
    };

    let spacing_x = 2;
    let spacing_y = 1;
    let total_spacing_x = spacing_x * (cols.saturating_sub(1));
    let total_spacing_y = spacing_y * (rows.saturating_sub(1));

    let available_width = target_width.saturating_sub(total_spacing_x as u16);
    let available_height = target_height.saturating_sub(total_spacing_y as u16);

    let cell_width = available_width / cols as u16;
    let cell_height = available_height / rows as u16;

    let (frame_width, frame_height) =
        calculate_frame_dimensions(cell_width, cell_height, aspect_ratio);

    temp_buffers.resize(count, String::new());

    let estimated_size = if color_enabled {
        (frame_width * frame_height * 15) as usize
    } else {
        (frame_width * frame_height * 2) as usize
    };

    for buffer in temp_buffers.iter_mut().take(count) {
        buffer.reserve(estimated_size);
    }

    for (i, frame) in frames.iter().enumerate() {
        if let Ok(()) = frame.to_ascii_with_buffer(
            color_enabled,
            true_color,
            frame_width as i32,
            frame_height as i32,
            &mut temp_buffers[i],
        ) {
            let centered = center_in_cell(&temp_buffers[i], cell_width, cell_height);
            temp_buffers[i] = centered;
        }
    }

    let content = combine_into_grid(&temp_buffers[..count], cols, spacing_x, spacing_y);
    let centered = center_full_grid(&content, true_width, true_height);
    ascii_buffer.push_str(&centered);
}

fn optimal_two_frame_layout(width: u16, height: u16, aspect_ratio: f64) -> (usize, usize) {
    let spacing_x = 2;
    let spacing_y = 1;

    let half_height = height.saturating_sub(spacing_y) / 2;
    let half_width = width.saturating_sub(spacing_x) / 2;

    let (vw, vh) = calculate_frame_dimensions(width, half_height, aspect_ratio);
    let (hw, hh) = calculate_frame_dimensions(half_width, height, aspect_ratio);

    let vertical_area = vw as usize * vh as usize;
    let horizontal_area = hw as usize * hh as usize;

    if vertical_area >= horizontal_area {
        (1, 2)
    } else {
        (2, 1)
    }
}

fn calculate_optimal_grid(
    count: usize,
    width: u16,
    height: u16,
    aspect_ratio: f64,
) -> (usize, usize) {
    let mut best = (1, count);
    let mut best_area = 0;

    let max_cols = (count as f64).sqrt().ceil() as usize + 2;
    let max_cols = max_cols.min(count).min(width as usize / 10);

    for cols in 1..=max_cols {
        let rows = (count + cols - 1) / cols;
        let spacing_x = 2 * cols.saturating_sub(1);
        let spacing_y = rows.saturating_sub(1);

        let available_w = width.saturating_sub(spacing_x as u16);
        let available_h = height.saturating_sub(spacing_y as u16);

        let cell_w = available_w / cols as u16;
        let cell_h = available_h / rows as u16;

        if cell_w == 0 || cell_h == 0 {
            continue;
        }

        let (fw, fh) = calculate_frame_dimensions(cell_w, cell_h, aspect_ratio);
        let area = fw as usize * fh as usize;

        if area > best_area {
            best_area = area;
            best = (cols, rows);
        }
    }

    best
}

#[inline]
fn calculate_frame_dimensions(max_width: u16, max_height: u16, aspect_ratio: f64) -> (u16, u16) {
    const CHAR_WIDTH_TO_HEIGHT_RATIO: f64 = 2.0;

    let effective_aspect_ratio = aspect_ratio * CHAR_WIDTH_TO_HEIGHT_RATIO;

    let height_from_width = (max_width as f64 / effective_aspect_ratio) as u16;
    let width_from_height = (max_height as f64 * effective_aspect_ratio) as u16;

    if height_from_width <= max_height && width_from_height <= max_width {
        let area1 = max_width as usize * height_from_width as usize;
        let area2 = width_from_height as usize * max_height as usize;

        if area1 >= area2 {
            (max_width, height_from_width)
        } else {
            (width_from_height, max_height)
        }
    } else if height_from_width <= max_height {
        (max_width, height_from_width)
    } else if width_from_height <= max_width {
        (width_from_height, max_height)
    } else {
        (max_width, max_height)
    }
}

fn center_in_cell(frame: &str, cell_w: u16, cell_h: u16) -> String {
    let lines = frame.lines().collect::<Vec<_>>();
    let frame_h = lines.len();
    let pad_top = (cell_h as usize).saturating_sub(frame_h) / 2;
    let pad_bottom = cell_h as usize - pad_top - frame_h;

    let mut out = String::with_capacity(cell_h as usize * cell_w as usize);
    let empty = " ".repeat(cell_w as usize);

    for _ in 0..pad_top {
        out.push_str(&empty);
        out.push('\n');
    }

    for &line in &lines {
        let visible = count_visible_chars_fast(line);
        let pad_left = (cell_w as usize).saturating_sub(visible) / 2;
        let pad_right = cell_w as usize - pad_left - visible;
        out.push_str(&" ".repeat(pad_left));
        out.push_str(line);
        out.push_str(&" ".repeat(pad_right));
        out.push('\n');
    }

    for _ in 0..pad_bottom {
        out.push_str(&empty);
        out.push('\n');
    }

    out
}

fn combine_into_grid(frames: &[String], cols: usize, spacing_x: usize, spacing_y: usize) -> String {
    if frames.is_empty() {
        return String::new();
    }

    let spacing = " ".repeat(spacing_x);
    let mut result = String::new();

    for (row_idx, row) in frames.chunks(cols).enumerate() {
        if row_idx > 0 {
            result.extend(std::iter::repeat('\n').take(spacing_y));
        }

        let row_lines: Vec<Vec<&str>> = row.iter().map(|f| f.lines().collect()).collect();
        let max_lines = row_lines.iter().map(Vec::len).max().unwrap_or(0);

        for line in 0..max_lines {
            for (i, frame) in row_lines.iter().enumerate() {
                if i > 0 {
                    result.push_str(&spacing);
                }
                if let Some(&l) = frame.get(line) {
                    result.push_str(l);
                }
            }
            result.push('\n');
        }
    }

    result
}

fn center_full_grid(grid: &str, true_w: u16, true_h: u16) -> String {
    let lines: Vec<&str> = grid.lines().collect();
    let content_h = lines.len();
    let pad_top = (true_h as usize).saturating_sub(content_h) / 2;
    let pad_bottom = true_h as usize - pad_top - content_h;
    let mut out = String::with_capacity(true_w as usize * true_h as usize);
    let blank = " ".repeat(true_w as usize);

    for _ in 0..pad_top {
        out.push_str(&blank);
        out.push('\n');
    }

    for &line in &lines {
        let visible = count_visible_chars_fast(line);
        let pad_left = (true_w as usize).saturating_sub(visible) / 2;
        let pad_right = true_w as usize - pad_left - visible;
        out.push_str(&" ".repeat(pad_left));
        out.push_str(line);
        out.push_str(&" ".repeat(pad_right));
        out.push('\n');
    }

    for _ in 0..pad_bottom {
        out.push_str(&blank);
        out.push('\n');
    }

    out
}

pub fn detect_true_color() -> bool {
    let is_vscode = std::env::var("TERM_PROGRAM")
        .map(|val| val.contains("vscode"))
        .unwrap_or(false);

    if is_vscode {
        return false;
    }

    std::env::var("COLORTERM")
        .map(|val| val == "truecolor" || val == "24bit")
        .unwrap_or(false)
}

#[inline]
fn rgb_to_ansi256_fast(r: u8, g: u8, b: u8) -> u8 {
    if r == g && g == b {
        if r < 8 {
            return 16;
        }
        if r > 248 {
            return 231;
        }
        return (((r as u16 - 8) * 24) / 247) as u8 + 232;
    }

    let r_idx = ((r as u16 * 5) / 255) as u8;
    let g_idx = ((g as u16 * 5) / 255) as u8;
    let b_idx = ((b as u16 * 5) / 255) as u8;

    16 + 36 * r_idx + 6 * g_idx + b_idx
}

#[inline]
fn count_visible_chars_fast(s: &str) -> usize {
    let bytes = s.as_bytes();
    let mut count = 0;
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == b'\x1b' {
            i += 1;
            while i < bytes.len() && bytes[i] != b'm' {
                i += 1;
            }
            i += 1;
        } else {
            count += 1;
            i += match bytes[i] {
                0..=0x7F => 1,
                0xC0..=0xDF => 2,
                0xE0..=0xEF => 3,
                0xF0..=0xF7 => 4,
                _ => 1,
            };
        }
    }

    count
}
