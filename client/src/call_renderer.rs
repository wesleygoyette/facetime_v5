use std::sync::Arc;

use tokio::sync::Mutex;

use crate::ascii_converter::{AsciiConverter, Frame};

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
    let (frame_width, frame_height, _layout_rows) = if count == 1 {
        return ascii_converter
            .lock()
            .await
            .nibbles_to_ascii(frames[0], width, height)
            .to_string();
    } else if count == 2 {
        if (width as f64) * 0.38 < height as f64 {
            let mut conv = ascii_converter.lock().await;
            let top = conv
                .nibbles_to_ascii(frames[0], width, (height - 1) / 2)
                .to_string();
            let bottom = conv
                .nibbles_to_ascii(frames[1], width, (height - 1) / 2)
                .to_string();
            return format!("{}\n{}", top, bottom);
        } else {
            let mut conv = ascii_converter.lock().await;
            let left = conv
                .nibbles_to_ascii(frames[0], (width - 1) / 2, height - 1)
                .to_string();
            let right = conv
                .nibbles_to_ascii(frames[1], (width - 1) / 2, height - 1)
                .to_string();
            return frames_side_by_side_to_string(&left, &right);
        }
    } else {
        let rows = ((count + 1) / 2) as u16;
        ((width - 2) / 2, (height - rows + 1) / rows, rows)
    };

    let mut result = String::with_capacity(8192);
    let mut ascii_frames = Vec::with_capacity(count);

    {
        let mut conv = ascii_converter.lock().await;
        for &frame in frames {
            ascii_frames.push(
                conv.nibbles_to_ascii(frame, frame_width, frame_height)
                    .to_string(),
            );
        }
    }

    for (i, pair) in ascii_frames.chunks(2).enumerate() {
        if i > 0 {
            result.push_str("\n\n");
        }
        match pair {
            [left, right] => result.push_str(&frames_side_by_side_to_string(left, right)),
            [only] => result.push_str(only),
            _ => {}
        }
    }

    result
}

fn frames_side_by_side_to_string(frame1: &str, frame2: &str) -> String {
    let frame1_lines: Vec<&str> = frame1.lines().collect();
    let frame2_lines: Vec<&str> = frame2.lines().collect();
    let max_lines = frame1_lines.len().max(frame2_lines.len());

    let mut result = String::with_capacity(frame1.len() + frame2.len() + max_lines * 3);

    for i in 0..max_lines {
        if i > 0 {
            result.push('\n');
        }

        let line1 = frame1_lines.get(i).copied().unwrap_or("");
        let line2 = frame2_lines.get(i).copied().unwrap_or("");
        result.push_str(line1);
        result.push_str("  ");
        result.push_str(line2);
    }
    result
}
