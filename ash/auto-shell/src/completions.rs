//! Auto-completion module
//!
//! Re-exports completion logic from ash-core and provides reedline integration.

// Re-export everything from ash-core completions
pub use ash_core::completions::{
    auto, command, file, flag, provider, spec, types,
    Completion, CompletionKind,
    get_completions, get_completions_with_context,
    CompletionContext, CompletionProvider,
    CompletionSignature, CompletionArgument,
    CompletionSpec, SubcommandSpec, FlagSpec, ArgSpec,
    WhenCondition, CompletionSource, ParseMode,
};

// Plan 037 M2.2 (Plan 071 融合后归属 ash crate): the reedline Completer
// adapter (`ShellCompleter`) lives in `ash::frontend::completions_reedline`.
// The dep-free completion types/specs below stay here.

// Plan 041 M7: the frontend-agnostic completion engine — sinks the orchestration
// logic out of ShellCompleter so TUI (reedline) and GUI (Tauri) share one engine.
pub mod engine;

// Plan 315: three-tier spec loading + runtime probe helpers.
pub mod spec_tiers;

// External command completion definitions
pub mod definitions;

// Plan 032 M2: AI completion layer (LLM subcommand + NL→pipeline), via a
// background thread + static cache (see module docs for the async strategy).
pub mod ai_layer;
