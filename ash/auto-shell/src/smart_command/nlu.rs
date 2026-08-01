//! Natural-language routing for SmartCommands (Plan 029 §3.3).
//!
//! When the user runs `ash smart "<natural language>"`, this module asks the
//! local Ollama model (via [`SmartCommandRole`] + [`Agent`]) to pick a
//! SmartCommand and fill its arguments, returning a structured [`NluResult`]
//! that the executor runs. The model does NOT execute commands — it only
//! routes (picks name + args). This keeps the AI step cheap, fast, and
//! side-effect-free.
//!
//! ## Output format
//!
//! Local 7B models follow strict JSON poorly, so we ask for a two-line format
//! that's both structured and tolerant:
//!
//! ```text
//! COMMAND: git.finish-worktree
//! ARGS: fix bug
//! ```
//!
//! [`parse_nlu_output`] extracts these lines case-insensitively and tolerates
//! surrounding explanation text.

use std::sync::Arc;

use auto_ai_agent::agent::Agent;
use auto_ai_agent::Client;

use super::config::SmartCommandSpec;
use super::role::SmartCommandRole;

/// The model's routing decision: which SmartCommand to run, with what args.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NluResult {
    /// The chosen spec name (must match one of the specs shown to the model).
    pub command: String,
    /// Positional arguments to pass to the body (`$1`, `$2`, …).
    pub args: Vec<String>,
}

/// Build the system prompt listing the available SmartCommands and prescribing
/// the output format. Pure function — testable without a model.
pub fn build_nlu_prompt(specs: &[SmartCommandSpec]) -> String {
    let mut menu = String::new();
    for spec in specs {
        let args = if spec.args.is_empty() {
            String::new()
        } else {
            format!(" <{}>", spec.args.join("> <"))
        };
        menu.push_str(&format!("- {}{args}: {}\n", spec.name, spec.description));
    }
    format!(
        "You route a natural-language request to exactly one ash SmartCommand.\n\
         Available commands:\n\
         {menu}\n\
         Reply with EXACTLY two lines and nothing else:\n\
         COMMAND: <command-name>\n\
         ARGS: <space-separated arguments, or empty>\n\
         Pick the single best-matching command. If none fits, still pick the\n\
         closest and put the request in ARGS."
    )
}

/// Parse the model's text output into an [`NluResult`].
///
/// Looks for a `COMMAND:` line and an optional `ARGS:` line, case-insensitive.
/// Tolerates leading/trailing explanation text. Returns `Err` if no `COMMAND:`
/// line is found.
pub fn parse_nlu_output(output: &str) -> Result<NluResult, String> {
    let mut command: Option<String> = None;
    let mut args: Vec<String> = Vec::new();

    for line in output.lines() {
        let trimmed = line.trim();
        let lower = trimmed.to_lowercase();
        if lower.starts_with("command:") {
            if command.is_none() {
                // Take the value from the ORIGINAL line (preserve case), offset
                // by the "command:" prefix length.
                let value = trimmed["command:".len()..].trim();
                if !value.is_empty() {
                    command = Some(value.to_string());
                }
            }
        } else if lower.starts_with("args:") {
            let value = trimmed["args:".len()..].trim();
            if !value.is_empty() {
                args = value.split_whitespace().map(String::from).collect();
            }
        }
    }

    match command {
        Some(c) => Ok(NluResult {
            command: c,
            args,
        }),
        None => Err(format!(
            "could not parse routing result (no 'COMMAND:' line). Model output:\n{output}"
        )),
    }
}

/// Route a natural-language request to a SmartCommand + args via the local
/// Ollama model. Builds a [`SmartCommandRole`] listing all specs, runs a single
/// Agent turn, and parses the structured output.
///
/// `client` must already be constructed (synchronously) — see
/// [`crate::ai::block_on_async`] for why AiClient can't be built
/// inside an async context.
pub fn route(
    user_msg: &str,
    specs: &[SmartCommandSpec],
    client: Arc<dyn Client>,
) -> Result<NluResult, String> {
    if specs.is_empty() {
        return Err("no SmartCommands available to route to".into());
    }

    let prompt = build_nlu_prompt(specs);
    let role = SmartCommandRole::new(prompt, vec![]); // no tools — pure routing
    let mut agent = Agent::new(role, client);

    // Drive the async Agent::run on a one-shot tokio runtime (the CLI is sync).
    let result = crate::ai::block_on_async(async move { agent.run(user_msg).await })
        .map_err(|e| format!("NLU agent run failed: {}", e))?;

    let parsed = parse_nlu_output(&result.output)?;

    // Validate the chosen command actually exists.
    if !specs.iter().any(|s| s.name == parsed.command) {
        return Err(format!(
            "NLU picked '{}', which is not an available SmartCommand",
            parsed.command
        ));
    }

    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(name: &str, desc: &str, args: &[&str]) -> SmartCommandSpec {
        let mut s = SmartCommandSpec::new(name, desc);
        s.args = args.iter().map(|a| a.to_string()).collect();
        s
    }

    // ── build_nlu_prompt ───────────────────────────────────────────────

    #[test]
    fn prompt_lists_all_specs() {
        let specs = vec![
            spec("git.finish-worktree", "finish a worktree", &["message"]),
            spec("hello", "say hello", &[]),
        ];
        let p = build_nlu_prompt(&specs);
        assert!(p.contains("git.finish-worktree"), "prompt should name it");
        assert!(p.contains("hello"));
        assert!(p.contains("finish a worktree"), "prompt should describe");
        assert!(p.contains("<message>"), "prompt should show args");
        assert!(p.contains("COMMAND:"), "prompt should prescribe format");
        assert!(p.contains("ARGS:"));
    }

    #[test]
    fn prompt_handles_empty_specs_gracefully() {
        let p = build_nlu_prompt(&[]);
        // Still prescribes the format (route() rejects empty specs before this
        // matters, but the prompt itself shouldn't panic).
        assert!(p.contains("COMMAND:"));
    }

    // ── parse_nlu_output ───────────────────────────────────────────────

    #[test]
    fn parse_valid_with_args() {
        let r = parse_nlu_output("COMMAND: git.finish-worktree\nARGS: fix bug\n").unwrap();
        assert_eq!(r.command, "git.finish-worktree");
        assert_eq!(r.args, vec!["fix", "bug"]);
    }

    #[test]
    fn parse_valid_no_args() {
        let r = parse_nlu_output("COMMAND: hello\n").unwrap();
        assert_eq!(r.command, "hello");
        assert!(r.args.is_empty());
    }

    #[test]
    fn parse_case_insensitive_prefix() {
        let r = parse_nlu_output("command: Deploy\nargs: prod\n").unwrap();
        assert_eq!(r.command, "Deploy");
        assert_eq!(r.args, vec!["prod"]);
    }

    #[test]
    fn parse_tolerates_surrounding_text() {
        // Models sometimes add explanation; we still extract the structured lines.
        let out = "Sure!\nCOMMAND: hello world-cmd\n ARGS: one two\nDone.";
        let r = parse_nlu_output(out).unwrap();
        assert_eq!(r.command, "hello world-cmd");
        assert_eq!(r.args, vec!["one", "two"]);
    }

    #[test]
    fn parse_empty_args_line() {
        let r = parse_nlu_output("COMMAND: hello\nARGS:\n").unwrap();
        assert_eq!(r.command, "hello");
        assert!(r.args.is_empty());
    }

    #[test]
    fn parse_missing_command_errors() {
        assert!(parse_nlu_output("ARGS: foo\nno command here").is_err());
    }

    #[test]
    fn parse_empty_output_errors() {
        assert!(parse_nlu_output("").is_err());
    }

    #[test]
    fn parse_first_command_wins() {
        // If the model outputs two COMMAND lines (shouldn't, but be safe), take
        // the first.
        let r = parse_nlu_output("COMMAND: first\nCOMMAND: second\n").unwrap();
        assert_eq!(r.command, "first");
    }

    #[test]
    fn parse_preserves_command_case() {
        let r = parse_nlu_output("COMMAND: Git.Finish-Worktree\n").unwrap();
        assert_eq!(r.command, "Git.Finish-Worktree");
    }

    // ── route (empty-specs guard, no daemon needed) ────────────────────

    #[test]
    fn route_rejects_empty_specs() {
        // A client that's never used because we bail first.
        let client: Arc<dyn Client> = Arc::new(auto_ai_client::AiClient::with_url("http://0.0.0.0:0"));
        let result = route("do something", &[], client);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("no SmartCommands"));
    }
}
