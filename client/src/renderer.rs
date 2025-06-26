use crossterm::{
    QueueableCommand,
    cursor::{Hide, MoveTo, Show},
    style::Print,
};
use std::io::{BufWriter, Write, stdout};

pub struct Renderer {
    last_frame: Option<Vec<String>>,
    terminal_size: Option<(u16, u16)>,
    writer: BufWriter<std::io::Stdout>,
    cursor_hidden: bool,
}

impl Renderer {
    pub fn new() -> Self {
        Self {
            last_frame: None,
            terminal_size: None,
            writer: BufWriter::with_capacity(32768, stdout()),
            cursor_hidden: false,
        }
    }

    pub fn update_terminal(
        &mut self,
        new_content: &str,
        width: u16,
        height: u16,
        color_enabled: bool,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let size_changed = Some((width, height)) != self.terminal_size;

        if size_changed {
            self.terminal_size = Some((width, height));
            self.last_frame = None;
            self.clear_terminal()?;
        }

        let new_lines: Vec<String> = new_content
            .replace("\r\n", "\n")
            .lines()
            .map(|s| s.to_string())
            .collect();

        if !self.cursor_hidden {
            self.writer.queue(Hide)?;
            self.cursor_hidden = true;
        }

        let result = if color_enabled {
            self.render_colored(&new_lines)
        } else {
            self.render_plain(&new_lines)
        };

        self.writer.queue(Show)?;
        self.cursor_hidden = false;

        if result.is_ok() {
            self.last_frame = Some(new_lines);
        }
        self.writer.flush()?;
        result
    }

    fn render_plain(
        &mut self,
        new_lines: &[String],
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let last_lines = if let Some(lines) = self.last_frame.take() {
            lines
        } else {
            return self.render_full_frame(new_lines);
        };
        let first_diff = new_lines
            .iter()
            .zip(last_lines.iter())
            .position(|(new, old)| new != old)
            .unwrap_or_else(|| std::cmp::min(new_lines.len(), last_lines.len()));

        for (line_num, line) in new_lines.iter().enumerate().skip(first_diff) {
            self.writer
                .queue(MoveTo(0, line_num as u16))?
                .queue(Print(line))?;

            if let Some(old_line) = last_lines.get(line_num) {
                if line.len() < old_line.len() {
                    self.clear_to_end_of_line(line.len())?;
                }
            }
        }

        if new_lines.len() > last_lines.len() {
            for line_num in last_lines.len()..new_lines.len() {
                self.writer
                    .queue(MoveTo(0, line_num as u16))?
                    .queue(Print(&new_lines[line_num]))?;
            }
        }

        self.last_frame = Some(last_lines);
        self.position_cursor_at_end(new_lines)
    }

    fn render_colored(
        &mut self,
        new_lines: &[String],
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let last_lines = match &self.last_frame {
            Some(lines) => lines,
            None => return self.render_full_frame(new_lines),
        };

        if new_lines.len() != last_lines.len() {
            return self.render_full_frame(new_lines);
        }

        let diffs: Vec<_> = new_lines
            .iter()
            .enumerate()
            .filter(|(i, line)| last_lines.get(*i).map(|old| line != &old).unwrap_or(true))
            .collect();

        if diffs.len() > new_lines.len() / 3 {
            self.render_full_frame(new_lines)
        } else {
            for (line_num, line) in diffs {
                self.writer
                    .queue(MoveTo(0, line_num as u16))?
                    .queue(Print(line))?;
                self.clear_to_end_of_line(line.len())?;
            }
            self.position_cursor_at_end(new_lines)
        }
    }

    fn render_full_frame(
        &mut self,
        lines: &[String],
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.writer.queue(MoveTo(0, 0))?;

        for (line_num, line) in lines.iter().enumerate() {
            self.writer
                .queue(MoveTo(0, line_num as u16))?
                .queue(Print(line))?;
            self.clear_to_end_of_line(line.len())?;
        }

        if let Some(last_lines) = &self.last_frame {
            if last_lines.len() > lines.len() {
                for line_num in lines.len()..last_lines.len() {
                    self.writer
                        .queue(MoveTo(0, line_num as u16))?
                        .queue(Print(" ".repeat(last_lines[line_num].len())))?;
                }
            }
        }

        self.position_cursor_at_end(lines)
    }

    fn clear_to_end_of_line(
        &mut self,
        pos: usize,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if let Some((width, _)) = self.terminal_size {
            if pos < width as usize {
                self.writer
                    .queue(crossterm::style::Print(" ".repeat(width as usize - pos)))?;
            }
        }
        Ok(())
    }

    fn position_cursor_at_end(
        &mut self,
        lines: &[String],
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if let Some((width, height)) = self.terminal_size {
            let last_line_num = lines.len().saturating_sub(1) as u16;
            let last_line_len = lines.last().map(|l| l.len()).unwrap_or(0) as u16;
            self.writer.queue(crossterm::cursor::MoveTo(
                last_line_len.min(width.saturating_sub(1)),
                last_line_num.min(height.saturating_sub(1)),
            ))?;
        }
        Ok(())
    }

    fn clear_terminal(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.writer
            .queue(crossterm::terminal::Clear(
                crossterm::terminal::ClearType::All,
            ))?
            .queue(crossterm::cursor::MoveTo(0, 0))?;
        Ok(())
    }
}
