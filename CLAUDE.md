# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build/Test Commands
- Build: `cargo build`
- Run: `cargo run`
- Check: `cargo check`
- Test all: `cargo test`
- Test single: `cargo test test_name`
- Format: `cargo fmt`
- Lint: `cargo clippy`

## Code Style Guidelines
- Use Rust 2024 edition
- Follow standard Rust naming conventions (snake_case for functions/variables, CamelCase for types)
- GStreamer imports: `use gst::prelude::*;` 
- Error handling: Use Result and unwrap/expect with meaningful error messages
- File organization: Keep platform-specific code in separate modules with cfg attributes
- Maintain consistent 4-space indentation
- Group imports by standard library, external crates, then internal modules
- Use type annotations for function signatures but prefer type inference in function bodies