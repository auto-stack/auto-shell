//! ash-server binary — Plan 042 M1 placeholder.
//!
//! M2 will add the axum HTTP server here. For now this just spawns the worker
//! and exits, proving the crate compiles standalone.

fn main() {
    println!("ash-server: spawning Shell worker (M2 will add HTTP server)...");
    let _handle = ash_server::spawn();
    println!("ash-server: worker spawned. Press Ctrl+C to exit.");
    // Keep alive — the worker thread runs until the channel closes.
    // M2 will replace this with an axum server that holds the handle.
    loop {
        std::thread::sleep(std::time::Duration::from_secs(60));
    }
}
