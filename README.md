# WeSFU (Wesley's Selective Forwarding Unit)

**WeSFU** is a command-line-based, real-time video conferencing application written in Rust. It implements a Selective Forwarding Unit (SFU) server that manages connections via TCP and forwards video streams over UDP. Designed for low-latency performance, WeSFU uses `tokio` for asynchronous networking and `opencv` for camera capture and frame processing.

## Features

* **Command-Line Interface (CLI):** Lightweight and intuitive text-based interface.
* **SFU Architecture:** Central server handles user connections and selectively forwards video streams.
* **Asynchronous I/O with Tokio:** Scales to many concurrent participants.
* **Camera Integration with OpenCV:** Captures and encodes real-time video frames.
* **UDP Video Forwarding:** High-throughput, low-latency transport layer for streaming.
* **Room and Camera Commands:** Join rooms, switch cameras, and monitor users in real time.
* **ASCII Video Rendering:** Streams video as ASCII frames in terminal environments.

## Architecture

```
  [Client A] ---\
  [Client B] ----> [WeSFU Server] ---> [Client C]
  [Client N] ---/
```

* **TCP (Control Plane):** Room creation, user registration, camera switching.
* **UDP (Media Plane):** Frame chunking, transmission, and reconstruction.

## Example Session

```text
╔══ Connected to WeSFU (version 5) ══╗
║ Time: 2025-06-17 19:22:51          ║
║ Server: 213.188.199.174            ║
║ User: fast-lion6677                ║
║ Status: Connection OK              ║
╚════════════════════════════════════╝

Available Commands:
    - list users|rooms|cameras   : Lists users, rooms, or available cameras
    - switch camera [index]      : Switches to camera at index
    - create room <string>       : Creates a new room
    - delete room <string>       : Deletes a room
    - join room <string>         : Joins a specific room
    - help                       : Displays a list of available commands
    - exit                       : Quits the application
```

## Getting Started

### Prerequisites

* Rust (1.72 or newer)
* OpenCV (installed and linked on your system)
* macOS/Linux (tested), terminal emulator

### Build Instructions

Clone the repository:

```bash
git clone https://github.com/your-username/wesfu.git
cd wesfu
```

#### macOS Users

A convenience script is provided in the project root:

```bash
./build-client-macos.sh
```

This will build the client binary with appropriate OpenCV linkage and optimization settings for macOS.

#### Manual Build

To build manually:

```bash
cargo build --release
```

## Running the Application

### Server

```bash
cargo run --bin wesfu-server
```

### Client

```bash
cargo run --bin wesfu-client -- <server_ip>
```

## Dependencies

* [`tokio`](https://crates.io/crates/tokio) – Asynchronous runtime
* [`opencv`](https://crates.io/crates/opencv) – Camera capture and image processing
* [`crossterm`](https://crates.io/crates/crossterm) – Terminal rendering
* [`serde`](https://crates.io/crates/serde), [`bincode`](https://crates.io/crates/bincode) – Optional serialization

## Roadmap

* Adaptive bitrate control
* Secure signaling (TLS)