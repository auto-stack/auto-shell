//! SmartCommand — "few deterministic steps + one AI judgment" commands
//! (Plan 029 §3 / appendix A).
//!
//! A SmartCommand packages a shell workflow (an AutoLang `.ash` body that runs
//! deterministic steps via `system()`) with an optional AI judgment step. Users
//! invoke them by name (`ash smart run git.finish-worktree <args>`) or, later,
//! via natural language routed through the local-Ollama [`SmartCommandRole`].
//!
//! ## Module layout
//! - [`config`] — `SmartCommandSpec` + `.at` parsing/serialization
//! - [`loader`] — discover specs from `$CWD/smart/`, `~/.config/ash/smart/`
//! - [`executor`] — run a spec's `.ash` body with `$1/$2` args
//! - [`role`] — [`SmartCommandRole`] (the local-Ollama NLU role)
//! - [`cli`] — the `ash smart` subcommand handler

pub mod cli;
pub mod config;
pub mod executor;
pub mod loader;
pub mod role;

pub use role::SmartCommandRole;
