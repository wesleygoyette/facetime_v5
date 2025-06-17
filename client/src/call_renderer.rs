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

    let target_aspect_ratio = WIDTH as f64 / HEIGHT as f64;

    match frames.len() {
        1 => {
            render_single_frame(
                frames[0],
                ascii_converter,
                width,
                height,
                target_aspect_ratio,
            )
            .await
        }
        2 => render_two_frames(frames, ascii_converter, width, height, target_aspect_ratio).await,
        _ => {
            render_multiple_frames(frames, ascii_converter, width, height, target_aspect_ratio)
                .await
        }
    }
}

async fn render_single_frame(
    frame: &Frame,
    ascii_converter: Arc<Mutex<AsciiConverter>>,
    width: u16,
    height: u16,
    target_aspect_ratio: f64,
) -> String {
    let (frame_width, frame_height) =
        calculate_frame_dimensions(width, height, target_aspect_ratio);
    let frame_str = ascii_converter
        .lock()
        .await
        .nibbles_to_ascii(frame, frame_width, frame_height)
        .to_string();
    center_frame(&frame_str, width, height)
}

async fn render_two_frames(
    frames: &[&Frame],
    ascii_converter: Arc<Mutex<AsciiConverter>>,
    width: u16,
    height: u16,
    target_aspect_ratio: f64,
) -> String {
    // Calculate both layouts
    let vertical_spacing = 1;
    let horizontal_spacing = 2;

    let available_height_per_frame = (height.saturating_sub(vertical_spacing)) / 2;
    let (v_width, v_height) =
        calculate_frame_dimensions(width, available_height_per_frame, target_aspect_ratio);
    let vertical_area = v_width as u32 * v_height as u32;

    let available_width_per_frame = (width.saturating_sub(horizontal_spacing)) / 2;
    let (h_width, h_height) =
        calculate_frame_dimensions(available_width_per_frame, height, target_aspect_ratio);
    let horizontal_area = h_width as u32 * h_height as u32;

    let mut conv = ascii_converter.lock().await;

    if vertical_area >= horizontal_area {
        // Vertical layout
        let top = conv
            .nibbles_to_ascii(frames[0], v_width, v_height)
            .to_string();
        let bottom = conv
            .nibbles_to_ascii(frames[1], v_width, v_height)
            .to_string();
        drop(conv); // Release lock early

        let centered_top = center_frame(&top, width, available_height_per_frame);
        let centered_bottom = center_frame(&bottom, width, available_height_per_frame);

        format!("{}\n{}", centered_top, centered_bottom)
    } else {
        // Horizontal layout
        let left = conv
            .nibbles_to_ascii(frames[0], h_width, h_height)
            .to_string();
        let right = conv
            .nibbles_to_ascii(frames[1], h_width, h_height)
            .to_string();
        drop(conv); // Release lock early

        let centered_left = center_frame(&left, available_width_per_frame, height);
        let centered_right = center_frame(&right, available_width_per_frame, height);

        combine_frames_horizontally(&centered_left, &centered_right)
    }
}

async fn render_multiple_frames(
    frames: &[&Frame],
    ascii_converter: Arc<Mutex<AsciiConverter>>,
    width: u16,
    height: u16,
    target_aspect_ratio: f64,
) -> String {
    let count = frames.len();
    let (cols, rows) = calculate_optimal_grid(count, width, height, target_aspect_ratio);

    let horizontal_spacing = 2;
    let vertical_spacing = 1;
    let total_horizontal_spacing = horizontal_spacing * (cols - 1);
    let total_vertical_spacing = vertical_spacing * (rows - 1);

    let available_width_per_frame = (width.saturating_sub(total_horizontal_spacing)) / cols;
    let available_height_per_frame = (height.saturating_sub(total_vertical_spacing)) / rows;

    let (frame_width, frame_height) = calculate_frame_dimensions(
        available_width_per_frame,
        available_height_per_frame,
        target_aspect_ratio,
    );

    // Pre-allocate with estimated capacity
    let estimated_capacity = (frame_width as usize * frame_height as usize * count) + (count * 100);
    let mut ascii_frames = Vec::with_capacity(count);

    // Process all frames while holding the lock once
    {
        let mut conv = ascii_converter.lock().await;
        for &frame in frames {
            let frame_str = conv
                .nibbles_to_ascii(frame, frame_width, frame_height)
                .to_string();
            let centered_frame = center_frame(
                &frame_str,
                available_width_per_frame,
                available_height_per_frame,
            );
            ascii_frames.push(centered_frame);
        }
    }

    // Build result string
    let mut result = String::with_capacity(estimated_capacity);
    for row in 0..rows {
        if row > 0 {
            result.push('\n');
        }

        let start_idx = (row * cols) as usize;
        let end_idx = ((row + 1) * cols).min(count as u16) as usize;
        let row_frames = &ascii_frames[start_idx..end_idx];

        if row_frames.len() == 1 {
            result.push_str(&row_frames[0]);
        } else {
            combine_frames_in_row(&mut result, row_frames);
        }
    }

    result
}

#[inline]
fn calculate_frame_dimensions(
    max_width: u16,
    max_height: u16,
    target_aspect_ratio: f64,
) -> (u16, u16) {
    if max_width == 0 || max_height == 0 {
        return (0, 0);
    }

    const CHAR_HEIGHT_TO_WIDTH_RATIO: f64 = 2.0;

    let ideal_height_for_width =
        (max_width as f64 / target_aspect_ratio / CHAR_HEIGHT_TO_WIDTH_RATIO) as u16;
    let ideal_width_for_height =
        (max_height as f64 * target_aspect_ratio * CHAR_HEIGHT_TO_WIDTH_RATIO) as u16;

    if ideal_height_for_width <= max_height {
        (max_width, ideal_height_for_width)
    } else {
        (ideal_width_for_height.min(max_width), max_height)
    }
}

fn calculate_optimal_grid(
    count: usize,
    width: u16,
    height: u16,
    target_aspect_ratio: f64,
) -> (u16, u16) {
    if count <= 2 {
        return (count as u16, 1);
    }

    let mut best_area = 0u32;
    let mut best_layout = (1, count as u16);

    for cols in 1..=count {
        let rows = (count + cols - 1) / cols;
        let horizontal_spacing = 2 * (cols - 1);
        let vertical_spacing = rows - 1;

        let available_width_per_frame =
            width.saturating_sub(horizontal_spacing as u16) / cols as u16;
        let available_height_per_frame =
            height.saturating_sub(vertical_spacing as u16) / rows as u16;

        if available_width_per_frame == 0 || available_height_per_frame == 0 {
            continue;
        }

        let (frame_width, frame_height) = calculate_frame_dimensions(
            available_width_per_frame,
            available_height_per_frame,
            target_aspect_ratio,
        );

        let area = frame_width as u32 * frame_height as u32;
        if area > best_area {
            best_area = area;
            best_layout = (cols as u16, rows as u16);
        }
    }

    best_layout
}

fn center_frame(frame: &str, container_width: u16, container_height: u16) -> String {
    let lines: Vec<&str> = frame.lines().collect();
    if lines.is_empty() {
        return " ".repeat(container_width as usize * container_height as usize);
    }

    let frame_height = lines.len();
    let frame_width = lines
        .iter()
        .map(|line| line.chars().count())
        .max()
        .unwrap_or(0);

    let container_width = container_width as usize;
    let container_height = container_height as usize;

    let vertical_padding = container_height.saturating_sub(frame_height) / 2;

    // Pre-calculate capacity for better performance
    let estimated_capacity = container_width * container_height + container_height;
    let mut result = String::with_capacity(estimated_capacity);

    // Top padding
    for i in 0..vertical_padding {
        if i > 0 {
            result.push('\n');
        }
        result.extend(std::iter::repeat(' ').take(container_width));
    }

    // Content lines
    for (i, line) in lines.iter().enumerate() {
        if !result.is_empty() {
            result.push('\n');
        }

        let line_width = line.chars().count();
        let horizontal_padding = container_width.saturating_sub(line_width) / 2;
        let remaining_width = container_width.saturating_sub(horizontal_padding + line_width);

        result.extend(std::iter::repeat(' ').take(horizontal_padding));
        result.push_str(line);
        result.extend(std::iter::repeat(' ').take(remaining_width));
    }

    // Bottom padding
    let remaining_lines = container_height.saturating_sub(vertical_padding + frame_height);
    for _ in 0..remaining_lines {
        result.push('\n');
        result.extend(std::iter::repeat(' ').take(container_width));
    }

    result
}

#[inline]
fn combine_frames_horizontally(frame1: &str, frame2: &str) -> String {
    let frames = [frame1, frame2];
    combine_frames_in_row_slice(&frames)
}

fn combine_frames_in_row(result: &mut String, frames: &[String]) {
    if frames.is_empty() {
        return;
    }
    if frames.len() == 1 {
        result.push_str(&frames[0]);
        return;
    }

    let frame_lines: Vec<Vec<&str>> = frames.iter().map(|f| f.lines().collect()).collect();
    let max_lines = frame_lines
        .iter()
        .map(|lines| lines.len())
        .max()
        .unwrap_or(0);

    for line_idx in 0..max_lines {
        if line_idx > 0 {
            result.push('\n');
        }

        for (frame_idx, lines) in frame_lines.iter().enumerate() {
            if frame_idx > 0 {
                result.push_str("  ");
            }
            if let Some(line) = lines.get(line_idx) {
                result.push_str(line);
            }
        }
    }
}

fn combine_frames_in_row_slice(frames: &[&str]) -> String {
    if frames.is_empty() {
        return String::new();
    }
    if frames.len() == 1 {
        return frames[0].to_string();
    }

    let frame_lines: Vec<Vec<&str>> = frames.iter().map(|f| f.lines().collect()).collect();
    let max_lines = frame_lines
        .iter()
        .map(|lines| lines.len())
        .max()
        .unwrap_or(0);

    let estimated_capacity =
        frames.iter().map(|f| f.len()).sum::<usize>() + max_lines * frames.len() * 2;
    let mut result = String::with_capacity(estimated_capacity);

    for line_idx in 0..max_lines {
        if line_idx > 0 {
            result.push('\n');
        }

        for (frame_idx, lines) in frame_lines.iter().enumerate() {
            if frame_idx > 0 {
                result.push_str("  ");
            }
            if let Some(line) = lines.get(line_idx) {
                result.push_str(line);
            }
        }
    }

    result
}
