//! `ash ask "<nl>"` — natural-language to AutoLang (Plan 029 §6).
//!
//! The user describes a multi-step task in plain language. A cloud model
//! (tier Max) generates AutoLang source, runs it via the `eval_auto` tool,
//! sees the result or error, and self-corrects — all driven by the agent's
//! ReAct loop. This handles logic too complex for F3's single command or a
//! SmartCommand's deterministic body (`fn`/`while`/`try-catch`/`if`).
//!
//! Example:
//! ```bash
//! ash ask "count the .rs files in the current directory and print the total"
//! ```

use std::sync::Arc;

use auto_ai_agent::agent::{Agent, StreamEvent};
use auto_ai_agent::role_def::Role;
use auto_ai_agent::{Client, ModelTier};
use miette::Result;

use crate::ash_command_tool::{AshCommandShellThread, AshCommandTool, EvalAutoTool};
use crate::shell::Shell;

/// The cloud-model role for generating AutoLang. tier=Max (code generation
/// needs a strong model). Knows AutoLang syntax + the ash `system()` bridge.
struct AutoLangCoder;

const AUTOLANG_SCHOOLING: &str = "\
You are an AutoLang code generator for Ash (AutoShell), a shell like bash/fish.\n\
AutoLang is a small typed language. You write it to solve multi-step tasks.\n\
\n\
## AutoLang basics\n\
- Define functions: `fn name(args) { ... return expr }`\n\
- Variables: `var x = expr`\n\
- Control flow: `if cond { }`, `for item in collection { }`, `while cond { }`, `try { } catch(e) { }`\n\
- Strings (\"...\"), numbers, booleans, lists ([a, b]), maps ({k: v})\n\
- Return a value from the last expression (it becomes the result).\n\
\n\
## Calling the shell from AutoLang\n\
- `system(\"git status\")` runs a shell command, returns its stdout (string).\n\
- `system_status()` returns the last command's exit code (int).\n\
- `system(...)` runs under the session's security policy; dangerous commands\n\
 (rm -rf /, mkfs, shutdown, ...) are refused. Do not try to work around it.\n\
- Prefer the dedicated ash command tools (ls/cat/grep/...) for shell work —\n\
 they are safer and give structured results; use system() only when a task\n\
 has no matching tool.\n\
\n\
## How to work\n\
Use the `eval_auto` tool to run your code. If it errors, read the error,\n\
fix the code, and call `eval_auto` again. Iterate until it works, then give\n\
the user a one-line summary of the result.\n\
Keep scripts short and focused.";

impl Role for AutoLangCoder {
    fn name(&self) -> &str {
        "ash-autolang-coder"
    }
    fn system_prompt(&self) -> &str {
        AUTOLANG_SCHOOLING
    }
    fn model_tier(&self) -> ModelTier {
        ModelTier::Max
    }
    fn temperature(&self) -> f64 {
        0.2 // code gen: low creativity for reliability
    }
    fn max_turns(&self) -> usize {
        8 // generate → run → fix loop budget
    }
}

/// Entry point for `ash ask`. `args` is everything after `ask`
/// (e.g. `["count", "the", ".rs", "files"]`). `policy` is the CLI security
/// policy (Plan 071 M2/S-5: the agent's shell runs under it — `ash
/// --read-only ask ...` stays read-only).
pub fn run(args: &[String], policy: ash_core::security::SecurityPolicy) -> Result<()> {
    if args.is_empty() {
        eprintln!("usage: ash ask \"<what you want to do>\"");
        std::process::exit(2);
    }
    let task = args.join(" ");

    // Build the client synchronously (daemon probe blocks).
    let ai_client = auto_ai_client::AiClient::new().map_err(|e| {
        miette::miette!(
            "AI client init: {}\n  (start the aaid daemon or set an API key)",
            e
        )
    })?;
    let client: Arc<dyn Client> = Arc::new(ai_client);

    // Dedicated shell thread backs both the eval_auto tool and the shell
    // command tools, sharing one session (cwd, definitions persist).
    let shell_thread = AshCommandShellThread::start_with_policy(policy);
    let tx = shell_thread.sender();

    let mut agent = Agent::new(AutoLangCoder, client);

    // Inject the live shell context (cwd / last command / aliases) so the
    // model knows the environment.
    let context = crate::ai::context::build_context_block(&Shell::new());
    agent.set_context(context);

    // Register tools: eval_auto (run AutoLang) + ash commands (system() backs).
    agent.register_tool(EvalAutoTool::new(tx.clone()));
    let signatures = Shell::new().registry().params();
    for sig in &signatures {
        if !sig.name.is_empty() {
            let desc = if sig.description.is_empty() {
                format!("ash command: {}", sig.name)
            } else {
                sig.description.clone()
            };
            agent.register_tool(AshCommandTool::new(sig.name.clone(), desc, tx.clone()));
        }
    }

    // Stream events: show the generated code + tool results inline.
    let on_event: Arc<dyn Fn(StreamEvent) + Send + Sync> = Arc::new(|ev| match ev {
        StreamEvent::Delta { text } => {
            use std::io::Write;
            print!("{text}");
            let _ = std::io::stdout().flush();
        }
        StreamEvent::ToolStart { tool, args } => {
            println!("\n  \x1b[2m\u{2699} {tool} {}\x1b[0m", crate::ai::brief::brief_args(&args));
        }
        StreamEvent::Tool { tool, result, .. } => {
            println!("\n  \x1b[2m\u{2190} {tool}: {}\x1b[0m", crate::ai::brief::brief_result(&result));
        }
        StreamEvent::Warning { text } => println!("\n  \x1b[2m\u{26a0}\u{fe0f} {text}\x1b[0m"),
        StreamEvent::Done { .. } => {}
        StreamEvent::Thinking { .. } => {}
        StreamEvent::Error { message } => println!("\n  [error] {message}"),
        StreamEvent::Cancelled { .. } => println!("\n  [cancelled]"),
    });

    let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let agent_result = crate::ai::block_on_async(async {
        agent.run_stream(&task, on_event, cancel).await
    })
    .map_err(|e| miette::miette!("ask failed: {}", e))?;

    println!(); // newline after the streamed reply
    let _ = agent_result;
    Ok(())
}
