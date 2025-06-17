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

    let count = frames.len();
    // Simple approach: just calculate the raw aspect ratio
    let target_aspect_ratio = WIDTH as f64 / HEIGHT as f64;

    if count == 1 {
        let (frame_width, frame_height) =
            calculate_frame_dimensions(width, height, target_aspect_ratio);
        let frame_str = ascii_converter
            .lock()
            .await
            .nibbles_to_ascii(frames[0], frame_width, frame_height)
            .to_string();
        return center_frame(&frame_str, width, height);
    }

    if count == 2 {
        return render_two_frames(frames, ascii_converter, width, height, target_aspect_ratio)
            .await;
    }

    render_multiple_frames(frames, ascii_converter, width, height, target_aspect_ratio).await
}

async fn render_two_frames(
    frames: &[&Frame],
    ascii_converter: Arc<Mutex<AsciiConverter>>,
    width: u16,
    height: u16,
    target_aspect_ratio: f64,
) -> String {
    // Try vertical layout (stacked)
    let vertical_spacing = 1; // Space between frames
    let available_height_per_frame = (height.saturating_sub(vertical_spacing)) / 2;
    let (vertical_frame_width, vertical_frame_height) =
        calculate_frame_dimensions(width, available_height_per_frame, target_aspect_ratio);

    // Try horizontal layout (side by side)
    let horizontal_spacing = 2; // Space between frames
    let available_width_per_frame = (width.saturating_sub(horizontal_spacing)) / 2;
    let (horizontal_frame_width, horizontal_frame_height) =
        calculate_frame_dimensions(available_width_per_frame, height, target_aspect_ratio);

    // Choose layout that gives larger frames (more area)
    let vertical_area = vertical_frame_width as u32 * vertical_frame_height as u32;
    let horizontal_area = horizontal_frame_width as u32 * horizontal_frame_height as u32;

    let mut conv = ascii_converter.lock().await;

    if vertical_area >= horizontal_area {
        // Use vertical layout
        let top = conv
            .nibbles_to_ascii(frames[0], vertical_frame_width, vertical_frame_height)
            .to_string();
        let bottom = conv
            .nibbles_to_ascii(frames[1], vertical_frame_width, vertical_frame_height)
            .to_string();

        // Center each frame in its allocated space
        let centered_top = center_frame(&top, width, available_height_per_frame);
        let centered_bottom = center_frame(&bottom, width, available_height_per_frame);

        format!("{}\n{}", centered_top, centered_bottom)
    } else {
        // Use horizontal layout
        let left = conv
            .nibbles_to_ascii(frames[0], horizontal_frame_width, horizontal_frame_height)
            .to_string();
        let right = conv
            .nibbles_to_ascii(frames[1], horizontal_frame_width, horizontal_frame_height)
            .to_string();

        // Center each frame in its allocated space and combine
        let centered_left = center_frame(&left, available_width_per_frame, height);
        let centered_right = center_frame(&right, available_width_per_frame, height);

        frames_side_by_side_to_string(&centered_left, &centered_right)
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

    // Calculate optimal grid layout
    let (cols, rows) = calculate_optimal_grid(count, width, height, target_aspect_ratio);

    // Calculate spacing
    let horizontal_spacing = 2;
    let vertical_spacing = 1;
    let total_horizontal_spacing = horizontal_spacing * (cols - 1);
    let total_vertical_spacing = vertical_spacing * (rows - 1);

    // Calculate frame dimensions
    let available_width_per_frame = (width.saturating_sub(total_horizontal_spacing)) / cols;
    let available_height_per_frame = (height.saturating_sub(total_vertical_spacing)) / rows;

    let (frame_width, frame_height) = calculate_frame_dimensions(
        available_width_per_frame,
        available_height_per_frame,
        target_aspect_ratio,
    );

    let mut result = String::with_capacity(8192);
    let mut ascii_frames = Vec::with_capacity(count);

    {
        let mut conv = ascii_converter.lock().await;
        for &frame in frames {
            let frame_str = conv
                .nibbles_to_ascii(frame, frame_width, frame_height)
                .to_string();
            // Center each frame in its allocated cell space
            let centered_frame = center_frame(
                &frame_str,
                available_width_per_frame,
                available_height_per_frame,
            );
            ascii_frames.push(centered_frame);
        }
    }

    // Arrange frames in grid
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
            result.push_str(&frames_row_to_string(row_frames));
        }
    }

    result
}

fn calculate_frame_dimensions(
    max_width: u16,
    max_height: u16,
    target_aspect_ratio: f64,
) -> (u16, u16) {
    if max_width == 0 || max_height == 0 {
        return (0, 0);
    }

    // Terminal characters are taller than wide, so we need to compensate
    // by using fewer rows relative to columns
    let char_height_to_width_ratio = 2.0; // typical terminal char is ~2x taller than wide

    // Calculate what height we should use for the given width to match target aspect ratio
    let ideal_height_for_width =
        (max_width as f64 / target_aspect_ratio / char_height_to_width_ratio) as u16;

    // Calculate what width we should use for the given height to match target aspect ratio
    let ideal_width_for_height =
        (max_height as f64 * target_aspect_ratio * char_height_to_width_ratio) as u16;

    // Choose the option that fits within our constraints
    if ideal_height_for_width <= max_height {
        // Width-constrained: use full width, calculate height
        (max_width, ideal_height_for_width)
    } else {
        // Height-constrained: use full height, calculate width
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

    // Try different grid configurations
    for cols in 1..=count {
        let rows = (count + cols - 1) / cols; // Ceiling division

        let horizontal_spacing = 2 * (cols - 1);
        let vertical_spacing = rows - 1;

        if horizontal_spacing >= width as usize || vertical_spacing >= height as usize {
            continue;
        }

        let available_width_per_frame = (width as usize - horizontal_spacing) / cols;
        let available_height_per_frame = (height as usize - vertical_spacing) / rows;

        if available_width_per_frame == 0 || available_height_per_frame == 0 {
            continue;
        }

        let (frame_width, frame_height) = calculate_frame_dimensions(
            available_width_per_frame as u16,
            available_height_per_frame as u16,
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
        return " "
            .repeat(container_width as usize)
            .repeat(container_height as usize);
    }

    let frame_height = lines.len();
    let frame_width = lines
        .iter()
        .map(|line| line.chars().count())
        .max()
        .unwrap_or(0);

    // Calculate vertical centering
    let container_height = container_height as usize;
    let vertical_padding = if container_height > frame_height {
        (container_height - frame_height) / 2
    } else {
        0
    };

    // Calculate horizontal centering
    let container_width = container_width as usize;
    let mut result = String::new();

    // Add top padding
    for _ in 0..vertical_padding {
        if !result.is_empty() {
            result.push('\n');
        }
        result.push_str(&" ".repeat(container_width));
    }

    // Add centered content lines
    for (i, line) in lines.iter().enumerate() {
        if !result.is_empty() {
            result.push('\n');
        }

        let line_width = line.chars().count();
        let horizontal_padding = if container_width > line_width {
            (container_width - line_width) / 2
        } else {
            0
        };

        // Left padding
        result.push_str(&" ".repeat(horizontal_padding));
        // Content
        result.push_str(line);
        // Right padding to fill the container width
        let remaining_width = container_width.saturating_sub(horizontal_padding + line_width);
        result.push_str(&" ".repeat(remaining_width));
    }

    // Add bottom padding
    let remaining_lines = container_height.saturating_sub(vertical_padding + frame_height);
    for _ in 0..remaining_lines {
        result.push('\n');
        result.push_str(&" ".repeat(container_width));
    }

    result
}

fn frames_side_by_side_to_string(frame1: &str, frame2: &str) -> String {
    frames_row_to_string(&[frame1.to_string(), frame2.to_string()])
}

fn frames_row_to_string(frames: &[String]) -> String {
    if frames.is_empty() {
        return String::new();
    }

    if frames.len() == 1 {
        return frames[0].clone();
    }

    let frame_lines: Vec<Vec<&str>> = frames.iter().map(|frame| frame.lines().collect()).collect();

    let max_lines = frame_lines
        .iter()
        .map(|lines| lines.len())
        .max()
        .unwrap_or(0);
    let mut result = String::with_capacity(
        frames.iter().map(|f| f.len()).sum::<usize>() + max_lines * frames.len() * 2,
    );

    for line_idx in 0..max_lines {
        if line_idx > 0 {
            result.push('\n');
        }

        for (frame_idx, frame_lines) in frame_lines.iter().enumerate() {
            if frame_idx > 0 {
                result.push_str("  "); // 2-space separator
            }

            let line = frame_lines.get(line_idx).copied().unwrap_or("");
            result.push_str(line);
        }
    }

    result
}
