//! SmartCommand role for ash's "few deterministic steps + one AI judgment"
//! commands (Plan 029).
//!
//! `SmartCommandRole` is an ash-specific role implemented against
//! [`auto_ai_agent::role_def::Role`]. Unlike the generic built-in roles
//! (Translator/Runner) that live in auto-ai, this one is bound to ash's
//! SmartCommand mechanism — it runs on a **local Ollama model**
//! (`model_tier = Min`, `preferred_provider = "ollama"`) for low-latency,
//! zero-cost NLU/parameter judgment, and is instantiated per SmartCommand
//! with a command-specific system prompt and tool allow-list.
//!
//! The Role trait is `pub`-exported by `auto-ai-agent`, so ash can define its
//! own domain roles without polluting auto-ai's shared built-in library.

use auto_ai_agent::role_def::Role;
use auto_ai_agent::ModelTier;

/// A role backing a single SmartCommand.
///
/// Construct it per SmartCommand with the command's system prompt (which
/// describes the deterministic prefix/suffix and what the AI must judge) and
/// the names of tools the AI step may call. The defaults pin it to a local
/// Ollama model (tier Min) for cheap, deterministic judgment.
pub struct SmartCommandRole {
    /// Command-specific system prompt — tells the model what to decide.
    system_prompt: String,
    /// Tool names the AI step may invoke (empty = all registered tools).
    allowed_tools: Vec<String>,
}

impl SmartCommandRole {
    /// Create a SmartCommand role with a custom system prompt.
    ///
    /// `allowed_tools` restricts which of the app's registered tools the AI
    /// judgment step can call; pass an empty vec to allow all.
    pub fn new(system_prompt: impl Into<String>, allowed_tools: Vec<String>) -> Self {
        Self {
            system_prompt: system_prompt.into(),
            allowed_tools,
        }
    }
}

impl Role for SmartCommandRole {
    fn name(&self) -> &str {
        "smart-command"
    }

    fn system_prompt(&self) -> &str {
        &self.system_prompt
    }

    /// Ultra-cheap tier — SmartCommand runs locally on Ollama for low-latency
    /// parameter judgment, not heavy reasoning.
    fn model_tier(&self) -> ModelTier {
        ModelTier::Min
    }

    /// Pin the provider to "ollama" so the daemon routes to the local model
    /// for this tier (the preferred_provider link, auto-ai side).
    fn preferred_provider(&self) -> Option<String> {
        Some("ollama".to_string())
    }

    /// Near-deterministic: SmartCommand judgment must be reproducible.
    fn temperature(&self) -> f64 {
        0.1
    }

    /// A SmartCommand's AI step is a single judgment — no long ReAct loops.
    fn max_turns(&self) -> usize {
        3
    }

    fn allowed_tools(&self) -> Vec<String> {
        self.allowed_tools.clone()
    }
}

/// Build a [`SmartCommandRole`] for a spec, rendering the spec's description
/// and argument names into the role's system prompt.
///
/// This is the NLU bridge: when a SmartCommand has an AI judgment step, the
/// executor constructs this role, wraps it in an `Agent`, and runs it to
/// decide parameters or branch on natural-language input. v1 returns the
/// role; the full Agent::run NLU flow (which needs a daemon) is wired up by
/// the caller. `allowed_tools` restricts what the AI step may invoke.
pub fn build_smart_role(
    spec: &super::config::SmartCommandSpec,
    allowed_tools: Vec<String>,
) -> SmartCommandRole {
    let prompt = format!(
        "You are the AI judgment step of the ash SmartCommand '{name}'.\n\
         {desc}\n\n\
         Positional arguments (by name): {args}.\n\
         Decide what the command should do based on the user's input, using the\n\
         allowed tools. Be concise and decisive.",
        name = spec.name,
        desc = spec.description,
        args = if spec.args.is_empty() {
            "(none)".to_string()
        } else {
            spec.args.join(", ")
        }
    );
    SmartCommandRole::new(prompt, allowed_tools)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_is_stable() {
        let role = SmartCommandRole::new("decide", vec![]);
        assert_eq!(role.name(), "smart-command");
    }

    #[test]
    fn system_prompt_is_injected() {
        let role = SmartCommandRole::new("You judge the deploy target.", vec![]);
        assert_eq!(role.system_prompt(), "You judge the deploy target.");
    }

    #[test]
    fn pins_local_ollama_provider_and_min_tier() {
        // The core SmartCommand contract: cheap, local, low-latency.
        let role = SmartCommandRole::new("x", vec![]);
        assert_eq!(role.preferred_provider().as_deref(), Some("ollama"));
        assert_eq!(role.model_tier(), ModelTier::Min);
    }

    #[test]
    fn near_deterministic_temperature_and_short_turns() {
        let role = SmartCommandRole::new("x", vec![]);
        assert!((role.temperature() - 0.1).abs() < 1e-9);
        assert_eq!(role.max_turns(), 3);
    }

    #[test]
    fn allowed_tools_passed_through() {
        let role = SmartCommandRole::new("x", vec!["git_status".into(), "read_file".into()]);
        assert_eq!(role.allowed_tools(), vec!["git_status", "read_file"]);
    }

    #[test]
    fn allowed_tools_empty_means_all() {
        // Empty vec is the documented "allow all" sentinel.
        let role = SmartCommandRole::new("x", vec![]);
        assert!(role.allowed_tools().is_empty());
    }

    /// The role must satisfy `dyn Role` so an Agent can consume it — this
    /// guards against accidental trait-method signature drift.
    #[test]
    fn is_usable_as_dyn_role() {
        let role = SmartCommandRole::new("x", vec![]);
        let r: &dyn Role = &role;
        assert_eq!(r.name(), "smart-command");
        assert_eq!(r.preferred_provider().as_deref(), Some("ollama"));
    }
}
