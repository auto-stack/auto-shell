//! AshPrompt — modular prompt engine, terminal-dependent half (Plan 037 M2.2)
//!
//! The dep-free halves (`config`, `context`) stayed in `auto_shell::prompt`.
//! This module holds the reedline/nu-ansi-term-dependent halves: the `AshPrompt`
//! engine (impls `reedline::Prompt`), `PromptModule`/`PromptSegment`, and the
//! `modules/`. They import `AshConfig`/`AshContext`/`GitInfo`/`GitStatus` back
//! across the crate boundary from `auto_shell::prompt`.

pub mod engine;
pub mod module;
pub mod modules;

pub use engine::AshPrompt;
pub use module::{PromptModule, PromptSegment, SegmentStyle};
