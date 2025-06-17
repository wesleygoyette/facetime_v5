use chrono::Local;
use crossterm::{
    cursor::MoveTo,
    execute,
    terminal::{Clear, ClearType},
};
use std::io::{Write, stdout};
use strum::IntoEnumIterator;

use crate::{camera::MAX_USER_CAMERAS, frame_generator::CameraTestMode};

pub struct CliDisplay;

impl CliDisplay {
    pub fn print_connected_message(server_addr: &str, username: &str) {
        let now = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

        let title = "Connected to WeSFU (version 5)";
        let lines = vec![
            format!("Time: {}", now),
            format!("Server: {}", server_addr),
            format!("User: {}", username),
            format!("Status: Connection OK"),
        ];

        Self::clear_screen();

        draw_double_box(title, &lines);

        Self::print_command_help();

        println!("Type a command to get started:\n");
    }

    pub fn print_user_list(user_list: &[String], current_username: &str) {
        let content = if user_list.is_empty() {
            vec!["(no users found)".to_string()]
        } else {
            user_list
                .iter()
                .map(|r| {
                    if r == current_username {
                        format!("- {} (you)", r)
                    } else {
                        format!("- {}", r)
                    }
                })
                .collect()
        };
        draw_box("Active Users", &content);
        println!();
    }

    pub fn print_room_list(room_list: &[String]) {
        let content = if room_list.is_empty() {
            vec!["(no rooms available)".to_string()]
        } else {
            room_list.iter().map(|r| format!("- {}", r)).collect()
        };
        draw_box("Available Rooms", &content);
        println!();
    }

    pub fn print_camera_list(camera_list: &[String], current_camera_index: i32) {
        let mut content = if camera_list.is_empty() {
            vec!["(no cameras available)".to_string()]
        } else {
            camera_list
                .iter()
                .map(|r| {
                    if r == "0" {
                        "- 0 (main camera)".to_string()
                    } else if let Ok(index) = r.parse::<i32>() {
                        let test_camera_start = MAX_USER_CAMERAS as i32;
                        let test_camera_end =
                            test_camera_start + CameraTestMode::iter().count() as i32;

                        if index >= test_camera_start && index < test_camera_end {
                            let test_index = (index - test_camera_start) as usize;
                            let test_mode = CameraTestMode::iter().nth(test_index);
                            if let Some(mode) = test_mode {
                                format!("- {} (test camera: {})", r, mode.to_string())
                            } else {
                                format!("- {} (test camera: unknown)", r)
                            }
                        } else {
                            format!("- {}", r)
                        }
                    } else {
                        format!("- {}", r)
                    }
                })
                .collect::<Vec<_>>()
        };

        content.push(String::new());

        content.push(format!(
            "You are currently using camera {}",
            current_camera_index
        ));

        draw_box("Available Cameras", &content);
        println!();
    }

    pub fn print_current_user_left_room(room_name: &str) {
        draw_box(
            "Disconnected",
            &[format!("You have left the room '{}'", room_name)],
        );
        println!();
    }

    pub fn print_prompt() {
        print!("> ");
        stdout().flush().unwrap();
    }

    pub fn print_command_help() {
        println!("\nAvailable Commands:");
        println!("    - list users|rooms|cameras   : Lists users, rooms, or available cameras");
        println!("    - switch camera [index]      : Switches to camera at index");
        println!("    - create room <string>       : Creates a new room");
        println!("    - delete room <string>       : Deletes a room");
        println!("    - join room <string>         : Joins a specific room");
        println!("    - help                       : Displays a list of available commands");
        println!("    - exit                       : Quits the application\n");
    }

    pub fn clear_screen() {
        let mut stdout = stdout();
        execute!(stdout, Clear(ClearType::All), MoveTo(0, 0)).unwrap();
    }
}

fn draw_double_box(title: &str, lines: &[String]) {
    let width = lines
        .iter()
        .map(|line| line.len())
        .max()
        .unwrap_or(0)
        .max(title.len() + 4);

    let horizontal = "═".repeat(width - title.len() - 2);
    println!("╔══ {} {}╗", title, horizontal);
    for line in lines {
        println!("║ {:width$} ║", line, width = width);
    }
    println!("╚{}╝", "═".repeat(width + 2));
}

fn draw_box(title: &str, lines: &[String]) {
    let left_padding = 2;
    let right_padding = 2;

    let content_width = lines.iter().map(|line| line.len()).max().unwrap_or(0);

    let inner_width = left_padding + content_width + right_padding;

    let title_str = format!(" {} ", title);
    let title_width = title_str.len();
    let border_fill_width = inner_width.max(title_width);

    let side_space = border_fill_width.saturating_sub(title_width);
    let top_border = format!(
        "╭─{:─<l$}{}{:─<r$}─╮",
        "",
        title_str,
        "",
        l = side_space / 2,
        r = side_space - (side_space / 2)
    );

    let bottom_border = format!("╰{:─<width$}╯", "", width = border_fill_width + 2);

    println!("\n{}", top_border);

    println!("│ {:width$} │", "", width = border_fill_width);

    for line in lines {
        println!(
            "│ {:<width$} │",
            format!(
                "{}{}{}",
                " ".repeat(left_padding),
                line,
                " ".repeat(right_padding)
            ),
            width = border_fill_width
        );
    }

    println!("{}", bottom_border);
}
