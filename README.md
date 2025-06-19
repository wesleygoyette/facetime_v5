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

* **TCP (Control Plane):** Room creation, user registration, camera switching.
* **UDP (Media Plane):** Frame chunking, transmission, and reconstruction.

## Demo

![Demo](assets/demo.gif)

## Getting Started

### Prerequisites

* Rust (1.72 or newer)
* OpenCV (installed and linked on your system)
* Terminal emulator

### Build Instructions

Clone the repository:

```bash
git clone https://github.com/wesleygoyette/facetime_v5
cd facetime_v5
```

#### macOS Users

A convenience script is provided in the project root to handle macOS-specific setup:

```bash
./build-client-macos.sh
```

This script:

* Installs **Rust** via `rustup` if it’s not already installed.
* Installs **Homebrew** if missing, then uses it to install:

  * `llvm` – for C++ headers and `libclang`
  * `opencv` – for camera capture and image processing
* Configures environment variables:

  * `CPATH` (if needed) for C++ standard headers
  * `LIBCLANG_PATH` and `DYLD_LIBRARY_PATH` for proper linking
* Builds the `client` binary in release mode using Cargo.

#### Manual Build

To build manually:

```bash
cargo build --release
```

## Running the Application

### Server

```bash
./target/release/server
```

### Client

```bash
./target/release/client
```

## Dependencies

* [`tokio`](https://crates.io/crates/tokio) – Asynchronous runtime
* [`opencv`](https://crates.io/crates/opencv) – Camera capture and image processing
* [`crossterm`](https://crates.io/crates/crossterm) – Terminal rendering

## Roadmap

* Adaptive bitrate control
* Secure signaling (TLS)
