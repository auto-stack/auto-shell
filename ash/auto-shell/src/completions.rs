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

// Plan 037 M2.2: the reedline Completer adapter (`ShellCompleter`) moved to
// the ash-tui crate (`ash_tui::completions_reedline`). The dep-free completion
// types/specs below stay here.

// Plan 315: three-tier spec loading + runtime probe helpers.
pub mod spec_tiers;

// External command completion definitions
pub mod definitions;

// Plan 032 M2: AI completion layer (LLM subcommand + NL→pipeline), via a
// background thread + static cache (see module docs for the async strategy).
pub mod ai_layer;
