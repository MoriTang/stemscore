# Project Instructions

This file provides context for AI assistants working on this project.

## Project Type: Rust CLI + Tauri 2 desktop audio application

### Build and test commands

```bash
cargo test --manifest-path cli/Cargo.toml
cargo clippy --manifest-path cli/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path desktop/src-tauri/Cargo.toml
cargo clippy --manifest-path desktop/src-tauri/Cargo.toml --all-targets -- -D warnings
npm --prefix desktop run build -- --bundles app
```


## Guidelines

- Follow existing code style and patterns
- Write tests for new functionality
- Keep changes focused and atomic
- Document public APIs

## Important Notes

- The default CLI and desktop app share the Rust/ONNX separation core.
- Python is optional and only used for explicit high-quality Demucs jobs.
- MIDI, MusicXML, PDF, and PyInstaller are intentionally out of scope.
- Keep the legacy `StemScore` model and output paths compatible unless a
  migration is implemented and tested.
