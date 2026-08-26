//! AshPrompt engine — assembles modules and implements reedline::Prompt
//!
//! Modules are rendered in parallel via rayon, then concatenated into
//! the final prompt string.
//!
//! **Performance**: Git info comes from a global background cache (context.rs).
//! Prompt rendering never does I/O — it only reads the cached git info
//! and formats strings. No TTL, no timers, no disk access during typing.

use rayon::prelude::*;
use std::borrow::Cow;

use auto_shell::prompt::config::AshConfig;
use auto_shell::prompt::context::AshContext;
use crate::prompt::module::PromptModule;
use crate::prompt::modules::{
    character::CharacterModule, cmd_duration::CmdDurationModule, directory::DirectoryModule,
    git_branch::GitBranchModule, git_status::GitStatusModule, status::StatusModule,
    time::TimeModule,
};

/// Modular prompt engine implementing `reedline::Prompt`.
///
/// Each prompt render:
/// 1. Builds an `AshContext` from current environment (no I/O — reads global git cache)
/// 2. Renders all registered modules in parallel (rayon)
/// 3. Concatenates segments into the final ANSI-colored string
///
/// No caching needed here — the expensive work (git discovery) is done by
/// the global GitCache in context.rs, triggered by cd and command execution.
pub struct AshPrompt {
    /// Left prompt modules (rendered left-to-right)
    modules: Vec<Box<dyn PromptModule>>,
    /// Right prompt modules
    right_modules: Vec<Box<dyn PromptModule>>,
    /// Prompt indicator character module
    character: Box<dyn PromptModule>,
    /// Prompt configuration
    config: AshConfig,
    /// Plan 070: continuation indicator matching the current symbol's display
    /// width (`▌# ` → `·· `), so wrapped/continuation lines align with the
    /// first input line. Updated by `set_character_symbol`.
    multiline_indicator: String,
}

impl AshPrompt {
    /// Create a new AshPrompt with default module set
    pub fn new(config: AshConfig) -> Self {
        let mut p = Self {
            modules: Vec::new(),
            right_modules: Vec::new(),
            character: Box::new(CharacterModule::new(&config)),
            multiline_indicator: Self::indicator_for(&config.module_string(
                "character", "success", "❯",
            )),
            config,
        };

        // Register left prompt modules (in display order)
        p.add_module(Box::new(DirectoryModule::new(&p.config)));
        p.add_module(Box::new(GitBranchModule::new(&p.config)));
        p.add_module(Box::new(GitStatusModule::new(&p.config)));
        p.add_module(Box::new(CmdDurationModule::new(&p.config)));
        p.add_module(Box::new(StatusModule::new(&p.config)));

        // Register right prompt modules
        p.add_right_module(Box::new(TimeModule::new(&p.config)));

        p
    }

    /// Add a module to the left prompt
    pub fn add_module(&mut self, module: Box<dyn PromptModule>) {
        if !self.config.is_module_disabled(module.name()) {
            self.modules.push(module);
        }
    }

    /// Add a module to the right prompt
    pub fn add_right_module(&mut self, module: Box<dyn PromptModule>) {
        if !self.config.is_module_disabled(module.name()) {
            self.right_modules.push(module);
        }
    }

    /// Plan 322: Override the character module's success symbol at runtime
    /// (used for mode switching: > / # / ? / ·). When locked, uses Blue color.
    pub fn set_character_symbol(&mut self, symbol: &str) {
        let locked = symbol.starts_with("▌");
        let clean = symbol.trim_start_matches("▌");
        self.multiline_indicator = Self::indicator_for(symbol);
        self.character = Box::new(CharacterModule::with_symbol_locked(clean, locked));
    }

    /// Plan 070: build a continuation indicator with the SAME display width as
    /// `{symbol} ` — `·` per symbol column plus the trailing space — so
    /// reedline's continuation lines line up under the first input line.
    fn indicator_for(symbol: &str) -> String {
        use unicode_width::UnicodeWidthStr;
        let width = UnicodeWidthStr::width(symbol);
        let dots = "·".repeat(width);
        format!("{dots} ")
    }

    /// Render left prompt (parallel module computation)
    fn render_left(&self, ctx: &AshContext) -> String {
        let newline_before = if self.config.add_newline { "\n" } else { "" };
        let segments: Vec<_> = self
            .modules
            .par_iter()
            .filter_map(|m| m.render(ctx))
            .collect();

        let content: String = segments
            .iter()
            .map(|s| s.to_ansi_string())
            .collect::<Vec<_>>()
            .join("");

        // Plan 036: multi-line prompt — move indicator to next line so
        // long directory paths don't squeeze the command input area.
        let newline_after = if self.config.multiline && !content.is_empty() {
            "\n"
        } else {
            ""
        };

        format!("{}{}{}", newline_before, content, newline_after)
    }

    /// Render right prompt (parallel module computation)
    fn render_right(&self, ctx: &AshContext) -> String {
        let segments: Vec<_> = self
            .right_modules
            .par_iter()
            .filter_map(|m| m.render(ctx))
            .collect();

        segments
            .iter()
            .map(|s| s.to_ansi_string())
            .collect::<Vec<_>>()
            .join("")
    }

    /// Render prompt indicator (❯)
    fn render_indicator(&self, ctx: &AshContext) -> String {
        self.character
            .render(ctx)
            .map(|s| s.to_ansi_string())
            .unwrap_or_else(|| "⟩ ".to_string())
    }

    /// Build context and render all prompt parts at once.
    /// No I/O — only reads from the global git cache.
    pub fn render_all(&self) -> (String, String, String) {
        let ctx = AshContext::from_current();
        let left = self.render_left(&ctx);
        let right = self.render_right(&ctx);
        let indicator = self.render_indicator(&ctx);
        (left, right, indicator)
    }
}

impl reedline::Prompt for AshPrompt {
    fn render_prompt_left(&self) -> Cow<'_, str> {
        let (left, _, _) = self.render_all();
        Cow::Owned(left)
    }

    fn render_prompt_right(&self) -> Cow<'_, str> {
        let (_, right, _) = self.render_all();
        Cow::Owned(right)
    }

    fn render_prompt_indicator(
        &self,
        _mode: reedline::PromptEditMode,
    ) -> Cow<'_, str> {
        let (_, _, indicator) = self.render_all();
        Cow::Owned(indicator)
    }

    fn render_prompt_multiline_indicator(&self) -> Cow<'_, str> {
        // Plan 070: width-matched to the current symbol so continuation lines
        // align with the first input line (was hardcoded "..> ").
        Cow::Borrowed(&self.multiline_indicator)
    }

    fn render_prompt_history_search_indicator(
        &self,
        search: reedline::PromptHistorySearch,
    ) -> Cow<'_, str> {
        Cow::Owned(format!("({}): ", search.term))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_ash_prompt_renders() {
        let prompt = AshPrompt::new(AshConfig::default());
        let ctx = AshContext::new(
            PathBuf::from("/tmp"),
            PathBuf::from("/home"),
            None,
            Some(0),
            AshConfig::default(),
        );
        let left = prompt.render_left(&ctx);
        // Should contain the directory
        assert!(!left.is_empty());
    }

    #[test]
    fn test_ash_prompt_indicator() {
        let prompt = AshPrompt::new(AshConfig::default());
        let ctx = AshContext::new(
            PathBuf::from("/tmp"),
            PathBuf::from("/home"),
            None,
            Some(0),
            AshConfig::default(),
        );
        let indicator = prompt.render_indicator(&ctx);
        assert!(indicator.contains("❯"));
    }

    #[test]
    fn test_reedline_prompt_trait() {
        use reedline::Prompt;
        let prompt = AshPrompt::new(AshConfig::default());
        let left = prompt.render_prompt_left();
        assert!(!left.is_empty());
        let _right = prompt.render_prompt_right();
        let indicator = prompt.render_prompt_indicator(reedline::PromptEditMode::Default);
        assert!(indicator.contains("❯"));
    }

    /// Plan 070: the continuation indicator must be exactly as wide as
    /// `{symbol} ` so continuation lines align under the first input line.
    #[test]
    fn test_multiline_indicator_matches_symbol_width() {
        use reedline::Prompt;
        use unicode_width::UnicodeWidthStr;

        let mut prompt = AshPrompt::new(AshConfig::default());
        for symbol in [">", "▌#", "?", "·", "▌>"] {
            prompt.set_character_symbol(symbol);
            let indicator = prompt.render_prompt_multiline_indicator();
            let expected = UnicodeWidthStr::width(symbol) + 1; // symbol + one space
            assert_eq!(
                UnicodeWidthStr::width(indicator.as_ref()),
                expected,
                "symbol {symbol:?}: indicator {indicator:?}"
            );
        }
    }
}
