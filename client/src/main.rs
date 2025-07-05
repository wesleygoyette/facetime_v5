mod audio_streamer;
mod call_interface;
mod camera;
mod cli_display;
mod client;
mod frame;
mod frame_generator;
mod jitter_buffer;
mod pre_call_interface;
mod renderer;
mod udp_handler;

use clap::Parser;
use rand::{Rng, rng, seq::IndexedRandom};

use crate::{camera::Camera, client::Client};

#[derive(Parser, Debug)]
struct Args {
    #[arg(short, long)]
    username: Option<String>,

    #[arg(short, long, default_value = "213.188.199.174")]
    server_address: String,

    #[arg(short, long, default_value = "0")]
    camera: String,

    #[arg(long, default_value_t = false)]
    color: bool,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();

    let username = match args.username {
        Some(username) => username,
        None => generate_username(),
    };

    let mut camera_index = match args.camera.parse() {
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

    if let Err(e) = Client::run(
        &args.server_address,
        &username,
        &mut camera_index,
        args.color,
    )
    .await
    {
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
