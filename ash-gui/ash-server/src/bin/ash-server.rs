//! ash-server binary — the standalone HTTP backend for the browser version.
//!
//! Spawns the Shell worker + an axum HTTP server. The browser version
//! (`npm run dev`) connects via vite proxy (`/api` → `localhost:3000`).
//!
//! Plan 070 M1 (S-1): binds **loopback by default**. The old `0.0.0.0:3000`
//! let anyone on the LAN execute commands via `/api/run_command`. To expose
//! the server beyond this machine, set `ASH_SERVER_BIND` — and a shared
//! secret via `ASH_SERVER_TOKEN` (mandatory for non-loopback binds; the
//! launcher passes the same env to the vite proxy, which injects it as a
//! Bearer header). Origin/Host checks apply in all modes — see `guard.rs`.
//!
//! Run: `cargo run -p ash-server` (from the ash-server workspace)

use std::net::SocketAddr;

#[tokio::main]
async fn main() {
    eprintln!("ash-server: spawning Shell worker...");
    let shell = ash_server::spawn();

    let bind = std::env::var("ASH_SERVER_BIND").unwrap_or_else(|_| "127.0.0.1:3000".into());
    let addr: SocketAddr = bind.parse().unwrap_or_else(|_| {
        eprintln!("ash-server: invalid ASH_SERVER_BIND '{bind}' (expected host:port)");
        std::process::exit(2);
    });
    if !addr.ip().is_loopback() {
        let tokenless = std::env::var("ASH_SERVER_TOKEN")
            .map(|t| t.trim().is_empty())
            .unwrap_or(true);
        if tokenless {
            eprintln!(
                "ash-server: refusing to bind non-loopback {addr} without ASH_SERVER_TOKEN \
                 (remote command execution would be unauthenticated)"
            );
            std::process::exit(2);
        }
        eprintln!(
            "ash-server: WARNING — listening on {addr} (non-loopback). \
             Anyone who obtains the token can run commands on this machine."
        );
    }

    eprintln!("ash-server: boot data ready, starting HTTP server on {bind}...");
    let app = ash_server::http::create_router(shell);

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .unwrap_or_else(|e| panic!("failed to bind {addr}: {e}"));
    eprintln!("ash-server: listening on http://{addr}");

    axum::serve(listener, app)
        .await
        .expect("server error");
}
