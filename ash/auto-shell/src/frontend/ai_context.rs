//! AI context builder (Plan 029 §2.3 + §7.2).
//!
//! Summarizes the shell's current state into a context block that's injected
//! into the system prompt of every AI feature (F3 NL→command, F4 chat,
//! SmartCommand NLU) so the model knows the user's environment without asking.
//!
//! Layers (per §7.2):
//! - **L0 static**: OS + cwd
//! - **L1 session**: last command + exit code
//! - **L2 aliases**: the user's shortcuts (first 5)
//! - L3 output is intentionally NOT included (v1 doesn't retain output).
//!
//! This is a pure function of `&Shell` — fully unit-testable without a daemon.

use crate::shell::Shell;

/// Build the AI context block from the shell's current state.
///
/// Inject this into a system prompt (F3) or an Agent's context (F4 via
/// `Agent::set_context`). Empty lines are skipped, so the block is compact.
pub fn build_context_block(shell: &Shell) -> String {
    let mut lines = Vec::new();

    // L0 — static environment.
    lines.push(format!("操作系统: {}", std::env::consts::OS));
    lines.push(format!("当前目录: {}", shell.pwd().display()));

    // L1 — last command + its exit code.
    if let Some(last) = shell.last_command_line() {
        lines.push(format!("上一条命令: {} (exit {})", last, shell.last_exit_code()));
    }

    // L2 — user aliases (preview the first 5 so the prompt stays small).
    let aliases = shell.aliases();
    if !aliases.is_empty() {
        let preview = aliases
            .iter()
            .take(5)
            .map(|(k, v)| format!("{}='{}'", k, v))
            .collect::<Vec<_>>()
            .join(", ");
        lines.push(format!("用户别名({} 个): {}", aliases.len(), preview));
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn includes_os_and_cwd() {
        let shell = Shell::new();
        let ctx = build_context_block(&shell);
        assert!(ctx.contains("操作系统:"), "should mention OS");
        assert!(ctx.contains("当前目录:"), "should mention cwd");
    }

    #[test]
    fn omits_last_command_when_none() {
        let shell = Shell::new();
        let ctx = build_context_block(&shell);
        // A fresh shell has run no commands.
        assert!(
            !ctx.contains("上一条命令"),
            "should not mention last command before any has run: {ctx}"
        );
    }

    #[test]
    fn omits_aliases_when_empty() {
        let shell = Shell::new();
        let ctx = build_context_block(&shell);
        assert!(
            !ctx.contains("用户别名"),
            "should not mention aliases when there are none: {ctx}"
        );
    }
}
