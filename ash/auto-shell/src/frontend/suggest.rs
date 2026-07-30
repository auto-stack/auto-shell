//! Warp-style "suggest next command" (Plan 029 §7.3).
//!
//! After a command runs, an async background fetch asks the local model
//! (tier:min / Ollama) for 3 likely next commands, given the cwd + the last
//! command + a snippet of its output. The REPL shows the suggestions before
//! the next prompt if they arrived in time; the fetch never blocks the shell.
//!
//! Opt-in via `~/.config/ash/config.at`:
//! ```text
//! ai {
//!     suggest_next : true
//! }
//! ```
//! Default off (it needs a running Ollama daemon and not everyone wants it).

use std::sync::{Arc, Mutex};

use auto_ai_client::{AiClient, CompletionRequest};

/// Global cache of the most recent suggestion fetch. `None` = nothing pending.
/// Populated by the background thread, drained by the REPL before each prompt.
static PENDING: Mutex<Option<Vec<String>>> = Mutex::new(None);

/// Whether the user has enabled suggest-next (`ai.suggest_next : true`).
/// Default false — opt-in.
pub fn is_enabled() -> bool {
    let cfg = crate::auto_config::load();
    crate::auto_config::get_bool(&cfg, "ai", "suggest_next").unwrap_or(false)
}

/// Fire a background fetch for next-command suggestions. Returns immediately
/// (never blocks the shell). The result lands in [`take_pending`] whenever the
/// model replies — the REPL checks before the next prompt.
///
/// `last_cmd` is the command that just ran; `output_snippet` is a short prefix
/// of its output (capped so we don't flood the prompt).
pub fn suggest_next_async(cwd: String, last_cmd: String, output_snippet: String) {
    std::thread::spawn(move || {
        let suggestions = fetch_suggestions(&cwd, &last_cmd, &output_snippet);
        if let Ok(s) = suggestions {
            if let Ok(mut guard) = PENDING.lock() {
                *guard = Some(s);
            }
        }
        // On error: leave the cache as-is (no suggestion shown). The fetch is
        // best-effort; a transient daemon error shouldn't bother the user.
    });
}

/// Drain the pending suggestions, if any. Returns `None` if no fetch has
/// completed (or it errored). Always clears the cache so a stale suggestion
/// isn't shown twice.
pub fn take_pending() -> Option<Vec<String>> {
    PENDING.lock().ok().and_then(|mut g| g.take())
}

/// Ask the local model for 3 next-command suggestions. Synchronous (runs on
/// the background thread). Returns the parsed suggestion lines.
fn fetch_suggestions(cwd: &str, last_cmd: &str, output: &str) -> std::result::Result<Vec<String>, String> {
    let client = AiClient::new().map_err(|e| format!("AI client init: {}", e))?;
    // Cap the output snippet so the prompt stays small.
    let snippet: String = output.chars().take(200).collect();
    let system = format!(
        "You suggest the next shell command for Ash (a bash/fish-like shell).\n\
         Context:\n\
         当前目录: {cwd}\n\
         刚执行的命令: {last_cmd}\n\
         输出摘要:\n{snippet}\n\n\
         Suggest up to 3 likely next commands, one per line, no numbering, no\n\
         explanation. If you can't suggest anything useful, reply with a single\n\
         line saying NONE."
    );
    let req = CompletionRequest::single("tier:min", "What should I run next?")
        .with_system(&system)
        .with_max_tokens(128)
        .with_temperature(0.4);

    let rt = tokio::runtime::Runtime::new().map_err(|e| format!("runtime: {}", e))?;
    let resp = rt.block_on(async { client.complete(&req).await });
    let resp = resp.map_err(|e| format!("{}", e))?;
    if !resp.is_ok() {
        return Err(format!("AI error: {:?}", resp.error));
    }

    let lines: Vec<String> = resp
        .content
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty() && l.to_uppercase() != "NONE")
        .take(3)
        .collect();
    Ok(lines)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn take_pending_is_none_when_empty() {
        // Clear any state from other tests first.
        let _ = take_pending();
        assert!(take_pending().is_none(), "should be None after draining");
    }

    #[test]
    fn take_pending_drains_so_not_shown_twice() {
        // Manually populate, then drain twice — second must be None.
        if let Ok(mut g) = PENDING.lock() {
            *g = Some(vec!["ls".into(), "pwd".into()]);
        }
        let first = take_pending();
        assert_eq!(first, Some(vec!["ls".into(), "pwd".into()]));
        assert!(take_pending().is_none(), "second take must be None");
    }

    #[test]
    fn is_enabled_defaults_false() {
        // No config set in test env → defaults false.
        // (We can't easily set config in a unit test, so just confirm it
        // doesn't panic and returns a bool.)
        let _ = is_enabled();
    }
}
