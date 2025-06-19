mod ascii_converter;
mod call_interface;
mod call_renderer;
mod camera;
mod cli_display;
mod client;
mod frame_generator;
mod pre_call_interface;
mod raw_mode_guard;
mod udp_handler;

use clap::Parser;
use rand::{Rng, rng, seq::IndexedRandom};

use crate::{camera::Camera, client::Client};

#[derive(Parser, Debug)]
struct Args {
    #[arg(short, long)]
    username: Option<String>,

    #[arg(short, long, default_value = "3.133.115.243")]
    server_address: String,

    #[arg(short, long, default_value = "0")]
    camera: String,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();

    let camera_index = match args.camera.parse() {
        Ok(idx) => idx,
        _ => {
            eprintln!("Invalid camera");
            return;
        }
    };

    if !Camera::is_valid_camera_name(&args.camera) {
        eprintln!("Camera not found");
        return;
    }

    let username = match args.username {
        Some(username) => username,
        None => generate_username(),
    };

    if let Err(e) = Client::run(&args.server_address, &username, camera_index).await {
        eprintln!("{}", e);
    }
}

fn generate_username() -> String {
    let adjectives = ["fast", "lazy", "cool", "smart", "brave"];
    let nouns = ["tiger", "eagle", "lion", "panda", "wolf"];

    let mut rng = rng();

    let adjective = adjectives.choose(&mut rng).unwrap();
    let noun = nouns.choose(&mut rng).unwrap();
    let number: u16 = rng.random_range(1..9999);

    format!("{}-{}{}", adjective, noun, number)
}
