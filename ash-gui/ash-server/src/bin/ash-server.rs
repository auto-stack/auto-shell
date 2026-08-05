//! ash-server binary — the standalone HTTP backend for the browser version.
//!
//! Spawns the Shell worker + an axum HTTP server on `0.0.0.0:3000`. The browser
//! version (`npm run dev`) connects via vite proxy (`/api` → `localhost:3000`).
//!
//! Run: `cargo run -p ash-server` (from the ash-server workspace)

use std::net::SocketAddr;

#[tokio::main]
async fn main() {
    eprintln!("ash-server: spawning Shell worker...");
    let shell = ash_server::spawn();

    eprintln!("ash-server: boot data ready, starting HTTP server on :3000...");
    let app = ash_server::http::create_router(shell);

    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("failed to bind :3000");
    eprintln!("ash-server: listening on http://localhost:3000");

    axum::serve(listener, app)
        .await
        .expect("server error");
}
