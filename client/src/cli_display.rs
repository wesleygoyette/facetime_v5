use crossterm::{
    cursor, execute,
    terminal::{Clear, ClearType},
};
use std::io::{Write, stdout};

pub const PROMPT_STR: &str = "> ";

pub struct CliDisplay;

impl CliDisplay {
    pub fn print_connected_message(server_addr: &str, username: &str) {
        println!("Connected to {} as '{}'!", server_addr, username);
    }

    pub fn print_user_list(user_list: &Vec<String>) {
        println!("Users:");

        for user in user_list {
            println!("  * {}", user);
        }
    }

    pub fn print_room_list(room_list: &Vec<String>) {
        println!("Rooms:");

        for room in room_list {
            println!("  * {}", room);
        }
    }

    pub fn print_current_user_left_room(room_name: &str) {
        let mut stdout = stdout();

        let _ = execute!(
            stdout,
            cursor::MoveToNextLine(1),
            Clear(ClearType::CurrentLine),
            cursor::MoveToColumn(0)
        );

        let _ = writeln!(stdout, "You have disconnected from '{}'", room_name);
        let _ = stdout.flush();
    }
}
