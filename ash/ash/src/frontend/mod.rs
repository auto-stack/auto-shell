//! frontend — ASH 的终端前端(原 ash-tui crate,Plan 071 Phase 2 融合迁入)
//!
//! 「线性输出 + 尾部动态」形态的终端侧实现:reedline 驱动的内联输入(动态
//! 尾部)、ratatui 结构化输出的一次性 Buffer→ANSI 线性打印(线性归档)、
//! 按需模态(070 底部脚本编辑器)、终端专属命令(less/more/color)与
//! 全屏子进程交接(subprocess)。建于 `auto-shell`(纯 Shell 逻辑)之上。
//!
//! Plan 037 M2.2 曾以独立 crate 承担"auto-shell 零终端依赖"的隔离;Plan 071
//! 退役双前端后该边界改由模块边界承担(auto-shell 依旧零终端依赖)。

pub mod block_header;
pub mod commands;
/// Plan 038 M3: fullscreen subprocess handoff (teardown/rebuild ratatui).
pub mod subprocess;
// `commands_less.rs` is the original `less`/`more` implementation (crossterm),
// moved verbatim; `commands` re-exports it and adds `color`.
mod commands_less;
pub mod completions_reedline;
/// Plan 070: the bottom-dynamic script editor modal (ratatui Inline viewport).
pub mod editor_overlay;
pub mod menu;
pub mod prompt;
pub mod renderer;
pub mod repl;
pub mod tail;
pub mod term;

// Re-export the entry-point type for the `ash` binary (composition root).
pub use repl::Repl;
