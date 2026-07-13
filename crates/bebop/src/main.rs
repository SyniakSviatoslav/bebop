//! Bebop native binary entry point. The real logic lives in the `bebop` lib
//! (so it is shared with the wasm build and unit-tested in one place).

fn main() {
    // Install structured telemetry (RUST_LOG=debug to see loop spans).
    bebop::init_tracing();
    bebop::cli::run();
}
