use core::error::Error;

use shared::tcp_command::TcpCommand;

pub struct TcpCommandHandler {}

impl TcpCommandHandler {
    pub async fn handle_command(incoming_command: &TcpCommand) -> Result<(), Box<dyn Error>> {
        dbg!(incoming_command);

        return Ok(());
    }
}
