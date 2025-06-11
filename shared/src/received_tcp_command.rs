use crate::tcp_command::TcpCommand;

pub enum ReceivedTcpCommand {
    EOF,
    Command(TcpCommand),
}
