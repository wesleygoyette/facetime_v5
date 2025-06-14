use core::error::Error;
use std::io::{self, BufRead, Write};

use shared::{
    RoomID, StreamID, received_tcp_command::ReceivedTcpCommand, tcp_command::TcpCommand,
    tcp_command_id::TcpCommandId,
};
use tokio::net::TcpStream;

use crate::cli_display::{CliDisplay, PROMPT_STR};

pub struct PreCallInterface;

impl PreCallInterface {
    pub async fn run(
        tcp_stream: &mut TcpStream,
    ) -> Result<Option<(String, Vec<u8>)>, Box<dyn Error + Send + Sync>> {
        let stdin = io::stdin();
        let mut reader = stdin.lock();

        loop {
            print!("{}", PROMPT_STR);
            io::stdout().flush()?;
            let mut line = String::new();
            if let Ok(n) = reader.read_line(&mut line) {
                if n == 0 {
                    return Ok(None);
                }

                let line = line.trim().to_lowercase();

                if line == "exit" {
                    println!("Exiting...");
                    return Ok(None);
                }

                let call_info_option = Self::handle_user_input(&line, tcp_stream).await?;

                if let Some(call_info) = call_info_option {
                    return Ok(Some(call_info));
                }
            }
        }
    }

    async fn handle_user_input(
        input: &str,
        tcp_stream: &mut TcpStream,
    ) -> Result<Option<(String, Vec<u8>)>, Box<dyn Error + Send + Sync>> {
        match input {
            "" => {}

            "create room" => {
                eprintln!("Usage: create room <string>");
            }
            command if command.starts_with("create room ") => {
                let command_parts: Vec<&str> = command.split(" ").collect();

                if command_parts.len() != 3 {
                    eprintln!("Usage: create room <string>");
                } else {
                    let room_name = command_parts[2];
                    create_room(tcp_stream, room_name).await?;
                }
            }

            "delete room" => {
                eprintln!("Usage: delete room <string>");
            }
            command if command.starts_with("delete room ") => {
                let command_parts: Vec<&str> = command.split(" ").collect();

                if command_parts.len() != 3 {
                    eprintln!("Usage: delete room <string>");
                } else {
                    let room_name = command_parts[2];
                    delete_room(tcp_stream, room_name).await?;
                }
            }

            "join room" => {
                eprintln!("Usage: join room <string>");
            }
            command if command.starts_with("join room ") => {
                let command_parts: Vec<&str> = command.split(" ").collect();

                if command_parts.len() != 3 {
                    eprintln!("Usage: join room <string>");
                } else {
                    let room_name = command_parts[2];
                    return join_room(tcp_stream, room_name).await;
                }
            }

            "list users" => {
                list_users(tcp_stream).await?;
            }

            "list rooms" => {
                list_rooms(tcp_stream).await?;
            }

            _ => {
                eprintln!("Unknown command");
            }
        }

        Ok(None)
    }
}

async fn list_users(tcp_stream: &mut TcpStream) -> Result<(), Box<dyn Error + Send + Sync>> {
    TcpCommand::Simple(TcpCommandId::GetUserList)
        .write_to_stream(tcp_stream)
        .await?;

    let received_command_option = TcpCommand::read_from_stream(tcp_stream).await?;

    let received_command = match received_command_option {
        ReceivedTcpCommand::EOF => {
            return Err("Unexpected EOF from server during list_users".into());
        }
        ReceivedTcpCommand::Command(command) => command,
    };

    let users = match received_command {
        TcpCommand::StringList(TcpCommandId::UserList, payload) => payload,
        _ => return Err("Invalid command from server during list_users".into()),
    };

    CliDisplay::print_user_list(&users);

    Ok(())
}

async fn list_rooms(tcp_stream: &mut TcpStream) -> Result<(), Box<dyn Error + Send + Sync>> {
    TcpCommand::Simple(TcpCommandId::GetRoomList)
        .write_to_stream(tcp_stream)
        .await?;

    let received_command_option = TcpCommand::read_from_stream(tcp_stream).await?;

    let received_command = match received_command_option {
        ReceivedTcpCommand::EOF => {
            return Err("Unexpected EOF from server during list_rooms".into());
        }
        ReceivedTcpCommand::Command(command) => command,
    };

    let rooms = match received_command {
        TcpCommand::StringList(TcpCommandId::RoomList, payload) => payload,
        _ => return Err("Invalid command from server during list_rooms".into()),
    };

    CliDisplay::print_room_list(&rooms);

    Ok(())
}

async fn create_room(
    tcp_stream: &mut TcpStream,
    room_name: &str,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    TcpCommand::String(TcpCommandId::CreateRoom, room_name.to_string())
        .write_to_stream(tcp_stream)
        .await?;

    let received_command_option = TcpCommand::read_from_stream(tcp_stream).await?;

    let received_command = match received_command_option {
        ReceivedTcpCommand::EOF => {
            return Err("Unexpected EOF from server during create_room".into());
        }
        ReceivedTcpCommand::Command(command) => command,
    };

    match received_command {
        TcpCommand::Simple(TcpCommandId::CreateRoomSuccess) => {
            println!("Successfully created room '{}'.", room_name);
            Ok(())
        }
        TcpCommand::String(TcpCommandId::ErrorResponse, error) => {
            eprintln!("{}", error);
            Ok(())
        }
        _ => Err("Invalid command from server during create_room".into()),
    }
}

async fn delete_room(
    tcp_stream: &mut TcpStream,
    room_name: &str,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    TcpCommand::String(TcpCommandId::DeleteRoom, room_name.to_string())
        .write_to_stream(tcp_stream)
        .await?;

    let received_command_option = TcpCommand::read_from_stream(tcp_stream).await?;

    let received_command = match received_command_option {
        ReceivedTcpCommand::EOF => {
            return Err("Unexpected EOF from server during delete_room".into());
        }
        ReceivedTcpCommand::Command(command) => command,
    };

    match received_command {
        TcpCommand::Simple(TcpCommandId::DeleteRoomSuccess) => {
            println!("Successfully deleted room '{}'.", room_name);
            Ok(())
        }
        TcpCommand::String(TcpCommandId::ErrorResponse, error) => {
            eprintln!("{}", error);
            Ok(())
        }
        _ => Err("Invalid command from server during delete_room".into()),
    }
}

async fn join_room(
    tcp_stream: &mut TcpStream,
    room_name: &str,
) -> Result<Option<(String, Vec<u8>)>, Box<dyn Error + Send + Sync>> {
    TcpCommand::String(TcpCommandId::JoinRoom, room_name.to_string())
        .write_to_stream(tcp_stream)
        .await?;

    let received_command_option = TcpCommand::read_from_stream(tcp_stream).await?;

    let received_command = match received_command_option {
        ReceivedTcpCommand::EOF => {
            return Err("Unexpected EOF from server during join_room".into());
        }
        ReceivedTcpCommand::Command(command) => command,
    };

    match received_command {
        TcpCommand::Bytes(TcpCommandId::JoinRoomSuccess, full_sid) => {
            if full_sid.len() != RoomID::default().len() + StreamID::default().len() {
                return Err("Unexpected payload length from server during join_room".into());
            }

            println!("Successfully joined room '{}'.", room_name);
            Ok(Some((room_name.to_string(), full_sid)))
        }
        TcpCommand::String(TcpCommandId::ErrorResponse, error) => {
            eprintln!("{}", error);
            Ok(None)
        }
        _ => Err("Invalid command from server during join_room".into()),
    }
}
