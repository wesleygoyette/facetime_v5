use crate::ascii_converter::{AsciiConverter, Frame, HEIGHT, WIDTH};
use std::sync::Arc;
use tokio::sync::Mutex;

pub async fn render_frames_to_string(
    frames: &[&Frame],
    ascii_converter: Arc<Mutex<AsciiConverter>>,
    width: u16,
    height: u16,
) -> String {
    if frames.is_empty() {
        return String::new();
    }

    let aspect_ratio = WIDTH as f64 / HEIGHT as f64;
    let count = frames.len();

    let (cols, rows) = match count {
        1 => (1, 1),
        2 => optimal_two_frame_layout(width, height, aspect_ratio),
        _ => calculate_optimal_grid(count, width, height, aspect_ratio),
    };

    let spacing_x = 2;
    let spacing_y = 1;

    let total_spacing_x = spacing_x * (cols - 1);
    let total_spacing_y = spacing_y * (rows - 1);

    let cell_width = width.saturating_sub(total_spacing_x as u16) / cols as u16;
    let cell_height = height.saturating_sub(total_spacing_y as u16) / rows as u16;

    let (frame_width, frame_height) =
        calculate_frame_dimensions(cell_width, cell_height, aspect_ratio);

    let mut ascii_frames = Vec::with_capacity(count);
    {
        let mut converter = ascii_converter.lock().await;
        for &frame in frames {
            let raw = converter
                .nibbles_to_ascii(frame, frame_width, frame_height)
                .to_string();
            let centered = center_in_cell(&raw, cell_width, cell_height);
            ascii_frames.push(centered);
        }
    }

    combine_into_grid(&ascii_frames, cols, spacing_x, spacing_y)
}

fn optimal_two_frame_layout(width: u16, height: u16, aspect_ratio: f64) -> (usize, usize) {
    let spacing_x = 2;
    let spacing_y = 1;

    let half_height = (height - spacing_y) / 2;
    let half_width = (width - spacing_x) / 2;

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

    for cols in 1..=count {
        let rows = (count + cols - 1) / cols;
        let spacing_x = 2 * (cols - 1);
        let spacing_y = rows - 1;

        let w = width.saturating_sub(spacing_x as u16) / cols as u16;
        let h = height.saturating_sub(spacing_y as u16) / rows as u16;

        if w == 0 || h == 0 {
            continue;
        }

        let (fw, fh) = calculate_frame_dimensions(w, h, aspect_ratio);
        let area = fw as usize * fh as usize;

        if area > best_area {
            best_area = area;
            best = (cols, rows);
        }
    }

    best
}

fn calculate_frame_dimensions(max_width: u16, max_height: u16, aspect_ratio: f64) -> (u16, u16) {
    const CHAR_RATIO: f64 = 2.0;
    let eff_ratio = aspect_ratio * CHAR_RATIO;

    let h_by_w = (max_width as f64 / eff_ratio).round() as u16;
    let w_by_h = (max_height as f64 * eff_ratio).round() as u16;

    match (h_by_w <= max_height, w_by_h <= max_width) {
        (true, false) => (max_width, h_by_w),
        (false, true) => (w_by_h, max_height),
        (true, true) => {
            let a1 = max_width as usize * h_by_w as usize;
            let a2 = w_by_h as usize * max_height as usize;
            if a1 >= a2 {
                (max_width, h_by_w)
            } else {
                (w_by_h, max_height)
            }
        }
        _ => (max_width.min(w_by_h), max_height.min(h_by_w)),
    }
}

fn center_in_cell(frame: &str, cell_w: u16, cell_h: u16) -> String {
    let lines: Vec<&str> = frame.lines().collect();
    let frame_h = lines.len();

    let pad_top = (cell_h as usize).saturating_sub(frame_h) / 2;
    let pad_bottom = cell_h as usize - pad_top - frame_h;

    let mut result = Vec::with_capacity(cell_h as usize);

    for _ in 0..pad_top {
        result.push(" ".repeat(cell_w as usize));
    }

    for &line in &lines {
        let len = line.chars().count();
        let pad_left = (cell_w as usize).saturating_sub(len) / 2;
        let pad_right = cell_w as usize - pad_left - len;

        let padded = format!("{}{}{}", " ".repeat(pad_left), line, " ".repeat(pad_right));
        result.push(padded);
    }

    for _ in 0..pad_bottom {
        result.push(" ".repeat(cell_w as usize));
    }

    result.join("\n")
}

fn combine_into_grid(frames: &[String], cols: usize, spacing_x: usize, spacing_y: usize) -> String {
    let mut lines = Vec::new();

    for chunk in frames.chunks(cols) {
        let row_lines: Vec<Vec<&str>> = chunk.iter().map(|f| f.lines().collect()).collect();
        let line_count = row_lines[0].len();

        for i in 0..line_count {
            let mut line = String::new();
            for (j, frame) in row_lines.iter().enumerate() {
                if j > 0 {
                    line.push_str(&" ".repeat(spacing_x));
                }
                line.push_str(frame.get(i).copied().unwrap_or(""));
            }
            lines.push(line);
        }

        if spacing_y > 0 && chunk.len() == cols {
            for _ in 0..spacing_y {
                lines.push("".to_string());
            }
        }
    }

    lines.join("\n")
}
