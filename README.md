# G-Streamer Stream Management System

A Rust-based stream management system for camera feeds using GStreamer, allowing for centralized management of video streams with multiple processing branches.

## Features

- **Centralized Stream Management**: Manage multiple camera streams from a single system
- **Dynamic Branch Creation**: Add recording, live viewing, or analytics branches to any stream
- **Resource Efficiency**: Prevent memory leaks and frame pile-up with efficient resource handling
- **Multiple Service Support**: Allow different services to access the same camera streams

## System Architecture

```
┌─────────────────┐       ┌───────────────────┐
│                 │       │                   │
│  Camera Manager ├───────► Stream Manager    │
│                 │       │                   │
└─────────────────┘       └─┬─────────────┬───┘
                            │             │
              ┌─────────────┴┐           ┌┴─────────────┐
              │              │           │              │
              │ Recording    │           │ Live Viewing │
              │ Service      │           │ Service      │
              │              │           │              │
              └──────────────┘           └──────────────┘
```

## Getting Started

### Prerequisites

- Rust toolchain (1.75+)
- GStreamer 1.20+ with development packages

### Building

```bash
cargo build
```

### Running

```bash
cargo run
```

### Running Examples

```bash
# Basic stream creation example
cargo run --example simple_stream

# Direct GStreamer examples:
cargo run --example rtsp_stream [rtsp_url]   # RTSP stream monitoring - logs frames and calculates FPS
cargo run --example rtsp_viewer [rtsp_url]   # RTSP stream viewer - displays the video in a window
cargo run --example rtsp_manager [rtsp_url]  # RTSP stream manager - recording with stream management

# Service-based examples (using our stream management framework):
cargo run --example rtsp_service [rtsp_url]     # Full service demonstration with recording
cargo run --example rtsp_live_view [rtsp_url]   # Live viewing service demonstration
cargo run --example rtsp_analytics [rtsp_url]   # Analytics service demonstration

All examples properly handle the macOS main loop requirements using the tutorial_common module.

# MacOS-specific examples (for troubleshooting GStreamer issues):
cargo run --example test_pattern          # Simple test pattern display - most reliable
cargo run --example macos_rtsp [rtsp_url] # Simplified RTSP player for macOS

See MACOS_TROUBLESHOOTING.md for help with common GStreamer issues on macOS.
```

If no RTSP URL is provided, a default public test stream will be used.

## Project Structure

```
src/
├── main.rs                  # Application entry point
├── stream_manager.rs        # Core stream management implementation
├── services/
│   ├── mod.rs               # Service module exports
│   ├── camera_manager.rs    # Camera discovery and management
│   ├── recording.rs         # Recording functionality
│   ├── streaming.rs         # Live streaming functionality
│   └── analytics.rs         # Video analytics
├── api/
│   ├── mod.rs               # API module exports
│   ├── rest.rs              # REST API implementation
│   └── websocket.rs         # WebSocket API for real-time events
└── config/
    └── mod.rs               # Configuration management
```

## Usage Examples

### Basic Stream Creation

```rust
// Create shared stream manager
let stream_manager = Arc::new(StreamManager::new());

// Add a camera stream
let source = StreamSource {
    stream_type: StreamType::Camera,
    uri: "/dev/video0".to_string(),
    name: "Webcam".to_string(),
    description: Some("Main webcam".to_string()),
};

let stream_id = stream_manager.add_stream(source)?;

// Add a recording branch
let recording_config = BranchConfig {
    branch_type: BranchType::Recording,
    output_path: Some("/recordings/video.mp4".to_string()),
    options: HashMap::new(),
};

let branch_id = stream_manager.add_branch(&stream_id, recording_config)?;
```

## License

MIT

## Acknowledgments

- GStreamer community
- Rust GStreamer bindings authors