mod tcp_command_handler;
mod tcp_handler;
mod udp_handler;
mod wes_sfu;

use log::{error, info};
use shared::{TCP_PORT, UDP_PORT};

use clap::Parser;

use crate::wes_sfu::WeSFU;

#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Args {
    #[arg(short, long, default_value = "0.0.0.0")]
    tcp: String,

    #[arg(short, long, default_value = "0.0.0.0")]
    udp: String,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();

    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let tcp_addr = format!("{}:{}", args.tcp, TCP_PORT);
    let udp_addr = format!("{}:{}", args.udp, UDP_PORT);

    let server = match WeSFU::bind(tcp_addr.clone(), udp_addr.clone()).await {
        Ok(wes_sfu_server) => wes_sfu_server,
        Err(e) => {
            error!("Error binding: {}", e);
            return;
        }
    };

    info!("WeSFU listening on TCP: {}, UDP: {}", tcp_addr, udp_addr);

    match server.listen().await {
        Ok(_) => (),
        Err(e) => {
            error!("{}", e);
            return;
        }
    };
}
