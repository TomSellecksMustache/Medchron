# Migration notes

This directory is a Rust reimplementation of the original Python prototype. The original Python source was not copied into this project to keep the application coherent. Behavior is intentionally narrowed to local rules-based extraction; the optional cloud LLM mode was omitted to support a clear local-only data-flow story.

Before use, run `cargo test` and validate compilation against the current stable Rust toolchain. Rust/Cargo is not installed in the generation environment, so compilation and integration tests could not be run here. Treat it as a first implementation requiring a local build/test pass.
