mod client;

use clap::Parser;
use rand::{Rng, rng, seq::IndexedRandom};

use crate::client::Client;

#[derive(Parser, Debug)]
struct Args {
    #[arg(short, long)]
    username: Option<String>,

    #[arg(short, long, default_value = "127.0.0.1")]
    server_address: String,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();

    let username = match args.username {
        Some(username) => username,
        None => generate_username(),
    };

    let _client = match Client::connect(&args.server_address, &username).await {
        Ok(client) => client,
        Err(e) => {
            eprintln!("Error connecting: {}", e);
            return;
        }
    };

    println!("Connected!");
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
