# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Structure
- Frontend: React + TypeScript application using Vite (in `/front-end`)
- Backend: Rust application using GStreamer (in parent directory)

## Backend Commands
- Build: `cargo build`
- Run: `cargo run`
- Check: `cargo check`
- Test all: `cargo test`
- Test single: `cargo test test_name`
- Format: `cargo fmt`
- Lint: `cargo clippy`

## Backend Style Guidelines
- Use Rust 2024 edition
- Follow standard Rust naming conventions (snake_case for functions/variables, CamelCase for types)
- GStreamer imports: `use gst::prelude::*;` 
- Error handling: Use Result and unwrap/expect with meaningful error messages
- File organization: Keep platform-specific code in separate modules with cfg attributes
- Maintain consistent 4-space indentation
- Group imports by standard library, external crates, then internal modules
- Use type annotations for function signatures but prefer type inference in function bodies

## Frontend Commands
- Development: `npm run dev`
- Build: `npm run build`
- Lint: `npm run lint`
- Preview: `npm run preview`

## Frontend Style Guidelines
- Use functional React components with hooks
- TypeScript with explicit type annotations
- Follow PascalCase for component files, camelCase for utility files
- Single quotes for strings, no semicolons
- Import organization with prettier-plugin-organize-imports