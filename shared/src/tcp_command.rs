use core::error::Error;
use std::str::from_utf8;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::{
    received_tcp_command::ReceivedTcpCommand, tcp_command_id::TcpCommandId,
    tcp_command_payload_type::TcpCommandPayloadType,
};

#[derive(Debug, Clone)]
pub enum TcpCommand {
    Simple(TcpCommandId),
    String(TcpCommandId, String),
    Bytes(TcpCommandId, Vec<u8>),
    StringList(TcpCommandId, Vec<String>),
}

impl TcpCommand {
    pub async fn write_to_stream<W>(
        &self,
        stream: &mut W,
    ) -> Result<(), Box<dyn Error + Send + Sync>>
    where
        W: AsyncWrite + Unpin,
    {
        match &self {
            TcpCommand::Simple(id) => {
                stream.write_all(&[id.to_byte()]).await?;
            }
            TcpCommand::String(id, payload) => {
                if payload.len() > u8::MAX as usize {
                    return Err("String payload too large".into());
                }

                let mut bytes = vec![id.to_byte(), payload.len() as u8];
                bytes.extend(payload.as_bytes());

                stream.write_all(&bytes).await?;
            }
            TcpCommand::Bytes(id, payload) => {
                if payload.len() > u8::MAX as usize {
                    return Err("Bytes payload too large".into());
                }

                let mut bytes = vec![id.to_byte(), payload.len() as u8];
                bytes.extend(payload);

                stream.write_all(&bytes).await?;
            }
            TcpCommand::StringList(id, payload) => {
                if payload.len() > u8::MAX as usize {
                    return Err("StringList payload too large".into());
                }

                let mut bytes = vec![id.to_byte(), payload.len() as u8];

                for str in payload {
                    if str.len() > u8::MAX as usize {
                        return Err("String in StringList payload too large".into());
                    }

                    bytes.push(str.len() as u8);
                    bytes.extend(str.as_bytes());
                }

                stream.write_all(&bytes).await?;
            }
        }

        Ok(())
    }

    pub async fn read_from_stream<R>(
        stream: &mut R,
    ) -> Result<ReceivedTcpCommand, Box<dyn Error + Send + Sync>>
    where
        R: AsyncRead + Unpin,
    {
        let mut buf = [0; 1];

        let first_byte = match stream.read(&mut buf).await {
            Ok(0) => return Ok(ReceivedTcpCommand::EOF),
            Ok(_) => buf[0],
            Err(e) => return Err(e.into()),
        };

        let command_id = TcpCommandId::from_byte(first_byte)?;

        match command_id.get_payload_type() {
            TcpCommandPayloadType::Simple => {
                Ok(ReceivedTcpCommand::Command(TcpCommand::Simple(command_id)))
            }
            TcpCommandPayloadType::String => {
                let mut payload_len_buf = [0];
                stream.read_exact(&mut payload_len_buf).await?;
                let payload_len = payload_len_buf[0] as usize;

                let mut payload_buf = vec![0; payload_len];
                stream.read_exact(&mut payload_buf).await?;
                let payload = from_utf8(&payload_buf)?.to_string();

                Ok(ReceivedTcpCommand::Command(TcpCommand::String(
                    command_id, payload,
                )))
            }
            TcpCommandPayloadType::Bytes => {
                let mut payload_len_buf = [0];
                stream.read_exact(&mut payload_len_buf).await?;
                let payload_len = payload_len_buf[0] as usize;

                let mut payload = vec![0; payload_len];
                stream.read_exact(&mut payload).await?;

                Ok(ReceivedTcpCommand::Command(TcpCommand::Bytes(
                    command_id, payload,
                )))
            }
            TcpCommandPayloadType::StringList => {
                let mut list_len_buf = [0];
                stream.read_exact(&mut list_len_buf).await?;
                let list_len = list_len_buf[0] as usize;

                let mut result = Vec::with_capacity(list_len);

                for _ in 0..list_len {
                    let mut str_len_buf = [0];
                    stream.read_exact(&mut str_len_buf).await?;
                    let str_len = str_len_buf[0] as usize;

                    let mut str_buf = vec![0; str_len];
                    stream.read_exact(&mut str_buf).await?;
                    let string = from_utf8(&str_buf)?.to_string();

                    result.push(string);
                }

                Ok(ReceivedTcpCommand::Command(TcpCommand::StringList(
                    command_id, result,
                )))
            }
        }
    }
}
