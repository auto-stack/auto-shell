//! SmartCommand — "few deterministic steps + one AI judgment" commands
//! (Plan 029 §3 / appendix A).
//!
//! A SmartCommand packages a shell workflow (an AutoLang `.ash` body that runs
//! deterministic steps via `system()`) with an optional AI judgment step. Users
//! invoke them by name or via natural language routed through the
//! [`SmartCommandRole`]. Plan 066/067(模式减法):`ash smart` 子命令与 GUI
//! 入口均已撤除 —— 用户裁定模式层面只留普通命令与 AI 两种,本模块连同
//! loader/nlu 作为未来「AI 模式内轻量 skill」形态的底件保留(暂无调用方)。
//!
//! ## Module layout
//! - [`config`] — `SmartCommandSpec` + `.at` parsing/serialization
//! - [`loader`] — discover specs from `$CWD/smart/`, `~/.config/ash/smart/`
//! - [`executor`] — run a spec's `.ash` body with `$1/$2` args
//! - [`nlu`] — natural-language routing (NL → spec + args) via SmartCommandRole
//! - [`role`] — [`SmartCommandRole`] (the local-Ollama NLU role)

pub mod config;
pub mod executor;
pub mod loader;
pub mod nlu;
pub mod role;

pub use role::SmartCommandRole;
