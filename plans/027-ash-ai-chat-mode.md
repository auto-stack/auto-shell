# 027 — ash AI Chat Mode (`?` mode)

**Status:** Design + Implementation Plan
**Date:** 2026-07-16
**Author:** ash
**Depends on:** 025 (REPL input modes), `auto-ai` (`auto-ai-client`)

## Goal

Add a persistent, multi-turn, streaming **AI chat** to `ash`, entered via a
locked `?` prompt. v1 is **chat-only — no tools**: pure conversational text.
We reuse infrastructure already present in `ash`:

- `auto-ai-client` crate (already a dependency, used by `ask_ai`).
- `tokio` (already `features = ["full"]`).
- The reserved `InputMode::AI` → prompt symbol `?` (Plan 025).
- The lock-mode mechanism F1/F2/F3 already use (`ModeState`, `▌` indicator).

## Out of scope (v1)

- Tool calling / the `auto-ai-agent` ReAct loop.
- Markdown rendering (output stays plain text, matching `auto-ai-cli`).
- A full-screen TUI chat view.
- A `?`-line-prefix trigger (only the locked mode via F4).
- A new config-file block for AI settings.

## User experience

1. **F4** (or **Alt+4**) → enter chat. Prompt becomes **`▌?`**.
2. Existing chat history is loaded from `~/.auto-shell-ai-chat.json`. A one-line
   banner is printed: `* 已恢复 N 轮对话 *` or `* 开始新对话 *`.
3. Each entered line is one chat turn. Enter appends the user message to the
   in-memory conversation, sends it to the daemon, and **streams tokens live to
   stdout** as they arrive. The assistant reply is appended to the conversation.
4. **Slash commands** (the only two implemented in v1):
   - `/clear` — forget the conversation (empty the in-memory history + truncate the file).
   - `/exit` — leave chat (same as the mode-switch exits below).
   - Anything else starting with `/` prints `未知命令: /xxx` and is a no-op.
   `/help` and other commands are deferred to a later plan. Empty line = no-op.
5. **Exit chat** happens when the user presses **Esc**, or any **mode-switch
   key**: F1/F2/F3 (or Alt+1/2/3). On exit, the conversation is saved to the
   JSON file. Esc returns to the unlocked `>` shell; a mode-switch key performs
   that mode switch. **Pressing F4/Alt+4 again while in chat also exits** the
   chat loop (toggle-style, same as F1/F2 toggle their locks), returning to the
   unlocked `>` shell.

The existing **F3 one-shot NL→command** flow (`ask_ai`) is **unchanged**.

## Architecture — dedicated `ai` module (Approach B)

New file **`src/frontend/ai.rs`** (registered in `frontend/mod.rs`). The
session type is intentionally unaware of `Repl`, so the module is unit-testable
without a network and so swapping in `auto-ai-agent` later is localized.

```rust
// src/frontend/ai.rs
use auto_ai_client::{AiClient, CompletionRequest, Message};
use std::path::PathBuf;

/// One persistent chat conversation, backed by a JSON file.
pub struct ChatSession {
    /// Conversation turns (user + assistant). Excludes the system prompt,
    /// which is rebuilt per request (cwd changes between sessions).
    messages: Vec<Message>,
    client: AiClient,
    history_path: PathBuf,
}

impl ChatSession {
    /// Load from ~/.auto-shell-ai-chat.json. Missing or corrupt file → empty.
    pub fn load() -> Result<Self, String>;

    /// Send one user turn. Streams deltas to stdout live. Appends the user
    /// message and the assistant reply to `messages`. Returns the full text.
    pub async fn send_turn_streaming(
        &mut self,
        user: &str,
        system: &str,
    ) -> Result<String, String>;

    pub fn turn_count(&self) -> usize;          // user turns stored
    pub fn clear(&mut self);                    // empty messages
    pub fn save(&self) -> Result<(), String>;
}

/// Build the system prompt from shell context. Pure fn, unit-testable.
pub fn build_system_prompt(cwd: &std::path::Path) -> String;

/// Run a future on a one-shot current-thread runtime (mirrors ask_ai).
pub fn block_on_async<F: std::future::Future>(fut: F) -> F::Output;

/// Is this input line a chat slash command? Returns the command if so.
pub fn parse_slash_command(line: &str) -> Option<SlashCommand>;

pub enum SlashCommand { Clear, Exit }
```

### Design notes

- **System prompt is per-request**, not stored in `messages`. `cwd` changes as
  the user runs `cd`, so a stored system message would go stale across
  sessions. `repl.rs` calls `build_system_prompt(self.shell.pwd())` each turn.
- **`ChatSession` is decoupled from `Repl`.** It takes a system string; it never
  touches shell state. This makes the module unit-testable and keeps the future
  `auto-ai-agent` swap contained to `ai.rs`.
- **`Vec<Message>` serialization "just works"** because `Message`/`ContentBlock`
  already derive `Serialize`/`Deserialize` (in `ai-config/src/wire.rs`). The
  file is a plain `serde_json` dump of `Vec<Message>`.

## Integration into `repl.rs` — standalone chat loop

Two changes in `repl.rs`:

### (a) New field + keybinding

```rust
pub struct Repl {
    // ... existing ...
    /// Lazy-initialized persistent AI chat session (None until first F4).
    chat: Option<crate::frontend::ai::ChatSession>,
}
```

F4 + Alt+4 keybindings, parallel to the existing F3 block. F4 inserts the
`\x15` prefix and submits (F1=`\x11`, F2=`\x12`, F3=`\x13`, so F4=`\x15`):

```rust
keybindings.add_binding(KeyModifiers::NONE, KeyCode::F(4),
    ReedlineEvent::Multiple(vec![
        ReedlineEvent::Edit(vec![EditCommand::InsertString("\x15".to_string())]),
        ReedlineEvent::Submit,
    ]));
// extend the Alt loop: ('4', "\x15")
```

### (b) Standalone chat loop in `run()`

When the outer `run()` loop sees a line starting with `\x15`, it calls a new
method that runs an **inner loop** and does not return to `run()` until chat
exits:

```rust
// in run()'s Ok(Signal::Success(line)) arm, after the \x13 (F3) branch:
if line.starts_with('\x15') {
    self.run_chat_loop()?;   // does not return until user exits chat
    continue;
}
```

`run_chat_loop`:

1. Lazily init `self.chat` (`ChatSession::load()`), print the resume/new banner.
2. Loop:
   - `read_line(&self.prompt)` (prompt is `▌?` — set locked = AI + `update_prompt()`).
   - If the line starts with a **chat-exit prefix** — `\x11`/`\x12`/`\x13`
     (F1/F2/F3 and their `Alt+1/2/3` aliases, which produce the same prefixes),
     `\x15` (F4/Alt+4, toggle-off), or `\x14` (Esc) — then `chat.save()` and
     **exit the loop**, handing control back to the outer `run()` loop:
     - For F1/F2/F3 prefixes, `run()` performs the corresponding mode switch
       (they already branch on these prefixes).
     - For F4 (`\x15`) and Esc (`\x14`), `run()` falls through to the unlocked
       `>` shell.
   - If the line is `/exit` → save and **exit the loop** (treated as Esc).
   - If the line is `/clear` → `chat.clear()`, `chat.save()`, print
     `* 对话已清空 *`, continue.
   - If the line is empty → no-op, continue.
   - Otherwise → one chat turn: `handle_chat_turn(&line)`.

`handle_chat_turn` (mirrors `ask_ai`'s runtime pattern):

```rust
fn handle_chat_turn(&mut self, user: &str) -> Result<()> {
    let session = self.chat.as_mut().unwrap();
    let system = crate::frontend::ai::build_system_prompt(&self.shell.pwd());
    match crate::frontend::ai::block_on_async(
        session.send_turn_streaming(user, &system),
    ) {
        Ok(_full) => { session.save().ok(); }
        Err(e) => eprintln!(
            "AI error: {e}\n(set ZHIPU_API_KEY / ANTHROPIC_API_KEY / \
             OPENAI_API_KEY or start aaid)"
        ),
    }
    Ok(())
}
```

Streaming inside `send_turn_streaming` uses
`client.complete_stream(&req, |ev| { ... print!("{}") ... })` — the same pattern
as `auto-ai-cli::chat_loop`. Each `delta` event prints its text inline; a
trailing newline is printed once on completion.

**Why standalone loop, not per-line routing:** it keeps all chat state/flow in
one method, gives an obvious place for the banner and save-on-exit, and means
the outer `run()` never has to know whether it's mid-chat. The tradeoff: the
chat loop owns the reedline editor for its duration (the user can't run shell
commands while chatting) — which is exactly the intended UX of a locked mode.

## Model & configuration

**Reuse `ask_ai`'s defaults — no new config.**

- Client: `AiClient::new()` (auto-discovers / lazy-starts the `aaid` daemon).
- Model: `"tier:mid"` (the daemon resolves it to e.g. glm-4.6).
- `max_tokens`: **4096** (chat answers need far more room than `ask_ai`'s 256-char command guess).
- `temperature`: **0.4** (slightly more conversational than `ask_ai`'s 0.3).
- System prompt (`build_system_prompt`), same shape as `ask_ai` but for
  conversation rather than single-command translation:

  > "You are an AI assistant for Ash (AutoShell), a shell similar to bash/fish.
  > The user's current directory is: {cwd}.
  > Answer the user's questions helpfully and concisely. You may discuss shell
  > commands, explain concepts, or help troubleshoot. Plain text only — no markdown."

## Error handling & interruption

- **No daemon / no API key** → `AiClient::new()` or `complete_stream` returns an
  error; we print the same one-line hint `ask_ai` uses (mentions the API key env
  vars or `aaid`). We **stay** in chat mode so the user can retry or `/exit`.
- **Corrupt JSON history** → `load()` recovers by starting from an empty array
  (logs a single line).
- **Ctrl+C mid-generation** → for v1 we rely on the daemon's HTTP timeout to
  bound the call; a `CtrlCGuard`-based interrupt is a follow-up polish. (The
  sync `block_on` blocks the REPL thread for the duration of one turn, same as
  `ask_ai` already does.)

## Testing strategy

Everything non-network is unit-testable because the module is decoupled:

- `build_system_prompt` — pure fn; assert it contains the `cwd` string.
- `ChatSession::load` / `save` / `clear` — use a tempdir; round-trip a
  `Message::user` / `Message::assistant` pair; cover the "file missing" and
  "file corrupt" recovery paths.
- `Vec<Message>` serde round-trip, including a multi-block message.
- `parse_slash_command` — covers `/clear`, `/exit`, `/CLEAR`, `  /clear  `,
  non-commands (return `None`).
- The streaming/network path is the only piece not unit-tested; it is verified
  manually (as `ask_ai` already is).

## File change summary

| File | Change |
|---|---|
| `src/frontend/ai.rs` | **NEW** — `ChatSession`, `build_system_prompt`, `block_on_async`, `parse_slash_command`, with unit tests (~250 lines). |
| `src/frontend/mod.rs` | Add `pub mod ai;` |
| `src/frontend/repl.rs` | Add `chat: Option<ChatSession>` field; F4 + Alt+4 keybindings; `\x15` branch in `run()` calling `run_chat_loop`; `run_chat_loop` + `handle_chat_turn`; save on exit. |
| `ash/auto-shell/Cargo.toml` | Add `serde_json` as a direct dependency (it's currently only transitive, via `auto-ai-client`/`ai-config`). |

Net: **1 new file + 3 small edits** (incl. one Cargo.toml line). Nothing in v1
is rewritten or deleted; `ask_ai` and the existing F3 flow are untouched.

## Future path (explicitly out of scope, but enabled)

Because `ChatSession` is an isolated boundary, adding tools/agent later means:
add `auto-ai-agent` to `Cargo.toml`, swap `ChatSession` for an `Agent` inside
`ai.rs` (register shell `Tool` impls), and nothing in `repl.rs` changes.
Likewise, a markdown renderer can later be layered into `send_turn_streaming`'s
delta handler without touching the REPL.

---

# Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:subagent-driven-development` (recommended) or `superpowers:executing-plans` to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a persistent, multi-turn, streaming AI chat to `ash`, entered via a locked `?` prompt (F4/Alt+4), backed by `auto-ai-client` and a JSON-persisted conversation.

**Architecture:** A new `src/frontend/ai.rs` module owns a `ChatSession` type (in-memory `Vec<Message>` + JSON load/save + streaming send). `src/frontend/repl.rs` gets a new F4/Alt+4 keybinding and a standalone `run_chat_loop()` method that owns the reedline editor for the duration of chat. Chat exits on Esc, F4 (toggle), or F1/F2/F3 (mode switch).

**Tech Stack:** Rust 2021, `auto-ai-client` (canonical wire types + `AiClient::complete_stream`), `tokio` (one-shot current-thread runtime), `serde_json` (history persistence), `reedline` (line editor + keybindings).

**Key API facts the plan relies on** (verified in the codebase):

- `auto_ai_client::{AiClient, CompletionRequest, Message, CompletionResponse}` — all re-exported from `ai-config`.
- `Message::user(text)` / `Message::assistant(text)` / `Message::system(text)` — constructors (`wire.rs:32-48`).
- `CompletionRequest { model, messages, max_tokens, temperature, system_prompt, tools, stream }` — all fields public (`wire.rs:156-168`); builders `with_system/with_max_tokens/with_temperature` exist.
- `AiClient::complete_stream(&req, on_event)` where `on_event: impl Fn(serde_json::Value) + Send + 'static`. Each event's delta text is at `value["text"]`; an error event has `value["type"]=="error"` + `value["message"]`. Returns `Result<CompletionResponse>`; `resp.content` is the full accumulated text, `resp.error` is `Option<String>` (`lib.rs:91-197`).
- Existing `Repl::ask_ai` (`repl.rs:340-394`) shows the exact one-shot runtime pattern to copy: `tokio::runtime::Builder::new_current_thread().enable_all().build()` + `rt.block_on(async { ... })`.
- F1/F2/F3 keybindings live in `repl.rs:170-213` (`add_common_keybindings`), inserting prefix chars `\x11`/`\x12`/`\x13` + `Submit`. The Alt loop at `repl.rs:204` binds Alt+1/2/3 to the same prefixes.

**Build/test commands:**
- Build the crate: `cargo build` (run from `D:/autostack/auto-shell/ash/auto-shell`, or `cargo build -p auto-shell` from the workspace root `D:/autostack/auto-shell/ash`).
- Run tests: `cargo test -p auto-shell`.
- Run only the new ai-module tests: `cargo test -p auto-shell frontend::ai`.

---

## Task 1: Create the `ai` module skeleton + register it + add `serde_json` dep

**Files:**
- Create: `ash/auto-shell/src/frontend/ai.rs`
- Modify: `ash/auto-shell/src/frontend/mod.rs`
- Modify: `ash/auto-shell/Cargo.toml`

- [ ] **Step 0: Add `serde_json` as a direct dependency**

`serde_json` is currently only available transitively (via `auto-ai-client` →
`ai-config`). Add it explicitly so `use serde_json;` compiles cleanly. In
`ash/auto-shell/Cargo.toml`, add alongside the other deps (match the version
that's already resolved transitively — `1.0`):

```toml
serde_json = "1.0"
```

Verify the build resolves it:

```bash
cargo build -p auto-shell
```

Expected: builds clean.

- [ ] **Step 1: Create `src/frontend/ai.rs` with module docs and a smoke test**

```rust
//! AI chat session for `ash`'s `?` mode (Plan 027).
//!
//! A `ChatSession` owns an in-memory conversation (`Vec<Message>`) and persists
//! it to `~/.auto-shell-ai-chat.json`. It is intentionally decoupled from the
//! REPL: callers build the system prompt and pass it in, so this module is
//! fully unit-testable without a network or a `Repl`.
//!
//! v1 is chat-only (no tools). See `plans/027-ash-ai-chat-mode.md`.

#[cfg(test)]
mod tests {
    #[test]
    fn smoke() {
        assert_eq!(2 + 2, 4);
    }
}
```

- [ ] **Step 2: Register the module in `frontend/mod.rs`**

Add `pub mod ai;` to `ash/auto-shell/src/frontend/mod.rs` so the module list
becomes:

```rust
pub mod renderer;
pub mod repl;
pub mod term;
pub mod completions_reedline;
pub mod ai;
```

- [ ] **Step 3: Build and run the smoke test**

Run: `cargo test -p auto-shell frontend::ai`
Expected: PASS (1 test: `smoke`).

- [ ] **Step 4: Commit**

```bash
git add ash/auto-shell/src/frontend/ai.rs ash/auto-shell/src/frontend/mod.rs ash/auto-shell/Cargo.toml
git commit -m "feat(ai): scaffold frontend::ai module + serde_json dep (Plan 027)"
```

---

## Task 2: `build_system_prompt` (pure fn, TDD)

**Files:**
- Modify: `ash/auto-shell/src/frontend/ai.rs`

- [ ] **Step 1: Write the failing test**

Add to the `tests` module:

```rust
use super::*;
use std::path::Path;

#[test]
fn system_prompt_contains_cwd() {
    let cwd = Path::new("/tmp/some-project");
    let s = build_system_prompt(cwd);
    assert!(s.contains("Ash"), "prompt should name Ash");
    assert!(s.contains("/tmp/some-project"), "prompt should include cwd");
    assert!(s.to_lowercase().contains("no markdown"), "prompt should forbid markdown");
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p auto-shell frontend::ai::tests::system_prompt_contains_cwd`
Expected: FAIL — `cannot find function build_system_prompt`.

- [ ] **Step 3: Implement `build_system_prompt`**

Add above the `tests` module:

```rust
use std::path::Path;

/// Build the per-request system prompt for the chat. Pure fn so it is unit-
/// testable and the cwd is always current (the user may `cd` between turns).
pub fn build_system_prompt(cwd: &Path) -> String {
    format!(
        "You are an AI assistant for Ash (AutoShell), a shell similar to bash/fish.\n\
         The user's current directory is: {}\n\
         Answer the user's questions helpfully and concisely. You may discuss shell\n\
         commands, explain concepts, or help troubleshoot. Plain text only — no markdown.",
        cwd.display()
    )
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p auto-shell frontend::ai::tests::system_prompt_contains_cwd`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add ash/auto-shell/src/frontend/ai.rs
git commit -m "feat(ai): build_system_prompt helper (Plan 027)"
```

---

## Task 3: `block_on_async` runtime helper (mirrors `ask_ai`)

**Files:**
- Modify: `ash/auto-shell/src/frontend/ai.rs`

- [ ] **Step 1: Write the failing test**

Add to the `tests` module:

```rust
#[test]
fn block_on_async_runs_future() {
    let val = block_on_async(async { 42 });
    assert_eq!(val, 42);
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p auto-shell frontend::ai::tests::block_on_async_runs_future`
Expected: FAIL — `cannot find function block_on_async`.

- [ ] **Step 3: Implement `block_on_async`**

Add above the `tests` module:

```rust
use std::future::Future;

/// Run a future on a fresh single-thread tokio runtime and block on it.
/// The REPL is synchronous; this mirrors `Repl::ask_ai`'s runtime pattern so
/// each chat turn can call the async `AiClient` without a global runtime.
pub fn block_on_async<F: Future>(fut: F) -> F::Output {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("failed to build tokio runtime");
    rt.block_on(fut)
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p auto-shell frontend::ai::tests::block_on_async_runs_future`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add ash/auto-shell/src/frontend/ai.rs
git commit -m "feat(ai): block_on_async runtime helper (Plan 027)"
```

---

## Task 4: `SlashCommand` parser (TDD)

**Files:**
- Modify: `ash/auto-shell/src/frontend/ai.rs`

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module:

```rust
#[test]
fn parse_slash_commands() {
    assert_eq!(parse_slash_command("/clear"), Some(SlashCommand::Clear));
    assert_eq!(parse_slash_command("/exit"), Some(SlashCommand::Exit));
    // Case-insensitive and whitespace-tolerant.
    assert_eq!(parse_slash_command("  /CLEAR  "), Some(SlashCommand::Clear));
    assert_eq!(parse_slash_command("/Exit"), Some(SlashCommand::Exit));
    // Not a command.
    assert_eq!(parse_slash_command("hello"), None);
    assert_eq!(parse_slash_command("/unknown"), None);
    assert_eq!(parse_slash_command(""), None);
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p auto-shell frontend::ai::tests::parse_slash_commands`
Expected: FAIL — `cannot find type/function`.

- [ ] **Step 3: Implement the parser and enum**

Add above the `tests` module:

```rust
/// The recognized chat slash commands (v1 minimal set).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlashCommand {
    /// Forget the conversation history.
    Clear,
    /// Leave chat mode (same as pressing Esc).
    Exit,
}

/// If `line` is one of the chat slash commands (case-insensitive, surrounding
/// whitespace ignored), return it. Otherwise return `None`.
pub fn parse_slash_command(line: &str) -> Option<SlashCommand> {
    match line.trim().to_lowercase().as_str() {
        "/clear" => Some(SlashCommand::Clear),
        "/exit" => Some(SlashCommand::Exit),
        _ => None,
    }
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p auto-shell frontend::ai::tests::parse_slash_commands`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add ash/auto-shell/src/frontend/ai.rs
git commit -m "feat(ai): slash command parser /clear /exit (Plan 027)"
```

---

## Task 5: `history_path()` helper (TDD with tempdir)

**Files:**
- Modify: `ash/auto-shell/src/frontend/ai.rs`

- [ ] **Step 1: Write the failing test**

Add to the `tests` module (uses `HOME` override so it works cross-platform and
in CI):

```rust
#[test]
fn history_path_under_home() {
    let tmp = std::env::temp_dir().join("ash_ai_history_path_test");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();

    let prev_home = std::env::var_os("HOME");
    // On Windows, dirs uses USERPROFILE; set both to be safe.
    let prev_profile = std::env::var_os("USERPROFILE");
    std::env::set_var("HOME", &tmp);
    std::env::set_var("USERPROFILE", &tmp);

    let p = history_path();
    assert_eq!(p.file_name().unwrap(), ".auto-shell-ai-chat.json");
    assert!(p.starts_with(&tmp), "history path should live under home dir");

    if let Some(h) = prev_home { std::env::set_var("HOME", h); } else { std::env::remove_var("HOME"); }
    if let Some(p) = prev_profile { std::env::set_var("USERPROFILE", p); } else { std::env::remove_var("USERPROFILE"); }
    let _ = std::fs::remove_dir_all(&tmp);
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p auto-shell frontend::ai::tests::history_path_under_home`
Expected: FAIL — `cannot find function history_path`.

- [ ] **Step 3: Implement `history_path`**

Add above the `tests` module:

```rust
use std::path::PathBuf;

/// Path to the persisted chat history: `~/.auto-shell-ai-chat.json`.
pub fn history_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".auto-shell-ai-chat.json")
}
```

> `dirs` is already a dependency of `auto-shell` (used in `repl.rs:49,107`).

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p auto-shell frontend::ai::tests::history_path_under_home`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add ash/auto-shell/src/frontend/ai.rs
git commit -m "feat(ai): history_path helper (Plan 027)"
```

---

## Task 6: `ChatSession` struct + `load`/`save`/`clear`/`turn_count` (TDD)

This is the core of the module. `send_turn_streaming` (the network call) comes
in Task 7; here we build the struct and all the non-network methods so they can
be tested against a tempdir.

**Files:**
- Modify: `ash/auto-shell/src/frontend/ai.rs`

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module. `ChatSession::with_history_path` is a test-only
constructor that points at an explicit file (the production constructor `load`
calls it with `history_path()`).

```rust
#[test]
fn load_from_missing_file_is_empty() {
    let tmp = std::env::temp_dir().join("ash_ai_missing_test");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let path = tmp.join("chat.json");
    assert!(!path.exists());

    let s = ChatSession::with_history_path(path.clone());
    assert_eq!(s.turn_count(), 0);
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn save_then_load_roundtrip() {
    let tmp = std::env::temp_dir().join("ash_ai_roundtrip_test");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let path = tmp.join("chat.json");

    let mut s = ChatSession::with_history_path(path.clone());
    s.push_user("hello");
    s.push_assistant("hi there");
    s.save().unwrap();
    assert!(path.exists(), "save should write the file");

    let s2 = ChatSession::with_history_path(path);
    assert_eq!(s2.turn_count(), 2);
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn load_from_corrupt_file_is_empty() {
    let tmp = std::env::temp_dir().join("ash_ai_corrupt_test");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let path = tmp.join("chat.json");
    std::fs::write(&path, "this is { not valid json").unwrap();

    let s = ChatSession::with_history_path(path.clone());
    assert_eq!(s.turn_count(), 0, "corrupt file should recover to empty");
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn clear_empties_and_persists() {
    let tmp = std::env::temp_dir().join("ash_ai_clear_test");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let path = tmp.join("chat.json");

    let mut s = ChatSession::with_history_path(path.clone());
    s.push_user("a");
    s.push_assistant("b");
    s.clear();
    assert_eq!(s.turn_count(), 0);
    s.save().unwrap();
    // Reload to confirm persistence.
    let s2 = ChatSession::with_history_path(path);
    assert_eq!(s2.turn_count(), 0);
    let _ = std::fs::remove_dir_all(&tmp);
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p auto-shell frontend::ai::tests`
Expected: FAIL — `cannot find type ChatSession`.

- [ ] **Step 3: Implement `ChatSession` with the non-network methods**

Add above the `tests` module:

```rust
use auto_ai_client::{AiClient, Message};

/// A persistent chat conversation backed by a JSON file.
///
/// `messages` holds only user+assistant turns; the system prompt is rebuilt
/// per request (cwd changes between sessions) and is NOT stored here.
pub struct ChatSession {
    messages: Vec<Message>,
    history_path: PathBuf,
}

impl ChatSession {
    /// Load the conversation from `~/.auto-shell-ai-chat.json`.
    /// Missing or corrupt file → empty conversation (recovers gracefully).
    pub fn load() -> Self {
        Self::with_history_path(history_path())
    }

    /// Construct from an explicit history file path (used by `load` and tests).
    pub fn with_history_path(path: PathBuf) -> Self {
        let messages = match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str::<Vec<Message>>(&text).unwrap_or_default(),
            Err(_) => Vec::new(),
        };
        ChatSession { messages, history_path: path }
    }

    /// Number of stored turns (user + assistant messages).
    pub fn turn_count(&self) -> usize {
        self.messages.len()
    }

    /// Append a user turn.
    pub fn push_user(&mut self, text: &str) {
        self.messages.push(Message::user(text));
    }

    /// Append an assistant turn.
    pub fn push_assistant(&mut self, text: &str) {
        self.messages.push(Message::assistant(text));
    }

    /// Forget the conversation (in-memory only; call `save` to persist).
    pub fn clear(&mut self) {
        self.messages.clear();
    }

    /// Serialize the conversation to the history file (overwrites).
    pub fn save(&self) -> std::io::Result<()> {
        let json = serde_json::to_string(&self.messages)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        std::fs::write(&self.history_path, json)
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p auto-shell frontend::ai::tests`
Expected: PASS — all tests including the 4 new `ChatSession` ones.

- [ ] **Step 5: Commit**

```bash
git add ash/auto-shell/src/frontend/ai.rs
git commit -m "feat(ai): ChatSession load/save/clear (Plan 027)"
```

---

## Task 7: `send_turn_streaming` (the network call)

This is the one method that touches the network; it is not unit-tested (matches
`ask_ai`). It reuses the verified `complete_stream` event format.

**Files:**
- Modify: `ash/auto-shell/src/frontend/ai.rs`

- [ ] **Step 1: Add the method to `ChatSession`**

Add inside `impl ChatSession` (and add `CompletionRequest` to the `use`):

```rust
use auto_ai_client::{AiClient, CompletionRequest, CompletionResponse, Message};
```

```rust
    /// Send one user turn. Appends the user message, builds a multi-turn
    /// request, streams the assistant reply to stdout as deltas arrive,
    /// appends the reply, and returns the full assistant text.
    ///
    /// `system` is the per-request system prompt (NOT stored in `messages`).
    pub async fn send_turn_streaming(
        &mut self,
        user: &str,
        system: &str,
    ) -> Result<String, String> {
        self.push_user(user);

        let client = AiClient::new().map_err(|e| format!("AI client init: {}", e))?;

        let req = CompletionRequest {
            model: "tier:mid".to_string(),
            messages: self.messages.clone(),
            max_tokens: Some(4096),
            temperature: Some(0.4),
            system_prompt: Some(system.to_string()),
            tools: Vec::new(),
            stream: false, // complete_stream sets this to true itself.
        };

        use std::io::Write;
        let on_event = |ev: serde_json::Value| {
            if let Some(text) = ev.get("text").and_then(|t| t.as_str()) {
                print!("{}", text);
                let _ = std::io::stdout().flush();
            }
        };

        let resp: CompletionResponse = client
            .complete_stream(&req, on_event)
            .await
            .map_err(|e| format!("{}", e))?;
        println!(); // newline after the streamed reply

        if let Some(err) = &resp.error {
            return Err(err.clone());
        }

        let text = resp.content.trim().to_string();
        self.push_assistant(&text);
        Ok(text)
    }
```

> Notes:
> - `AiClient::complete_stream`'s signature is `on_event: impl Fn(serde_json::Value) + Send + 'static`; our closure satisfies this and is stateless.
> - We construct a fresh `AiClient` per turn for simplicity (matching `ask_ai`). The daemon connection is cheap (a `reqwest::Client`); pooling is a later optimization.
> - The closure prints only `text` deltas; `done`/`error` events carry no text and are ignored here (errors surface via the returned `CompletionResponse.error`).

- [ ] **Step 2: Build to verify it compiles**

Run: `cargo build -p auto-shell`
Expected: compiles with no errors. (No new test — this is network code; it is verified manually in Task 10.)

- [ ] **Step 3: Commit**

```bash
git add ash/auto-shell/src/frontend/ai.rs
git commit -m "feat(ai): ChatSession::send_turn_streaming (Plan 027)"
```

---

## Task 8: Register F4/Alt+4 keybindings in `repl.rs`

**Files:**
- Modify: `ash/auto-shell/src/frontend/repl.rs` (the `add_common_keybindings` fn, around lines 186-213)

- [ ] **Step 1: Add the F4 keybinding after the F3 block**

After the existing F3 binding (which ends at `repl.rs:193`), insert:

```rust
            // Plan 027: F4 — enter persistent AI chat mode (insert \x15 + submit).
            keybindings.add_binding(
                KeyModifiers::NONE,
                KeyCode::F(4),
                ReedlineEvent::Multiple(vec![
                    ReedlineEvent::Edit(vec![EditCommand::InsertString("\x15".to_string())]),
                    ReedlineEvent::Submit,
                ]),
            );
```

- [ ] **Step 2: Extend the Alt-loop to include Alt+4**

Change the Alt loop at `repl.rs:204` from:

```rust
            for (key, prefix) in [('1', "\x11"), ('2', "\x12"), ('3', "\x13")] {
```

to:

```rust
            for (key, prefix) in [('1', "\x11"), ('2', "\x12"), ('3', "\x13"), ('4', "\x15")] {
```

- [ ] **Step 3: Build to verify it compiles**

Run: `cargo build -p auto-shell`
Expected: compiles with no errors.

- [ ] **Step 4: Commit**

```bash
git add ash/auto-shell/src/frontend/repl.rs
git commit -m "feat(repl): bind F4/Alt+4 to enter AI chat (Plan 027)"
```

---

## Task 9: Add `chat` field + `run_chat_loop` + `handle_chat_turn` in `repl.rs`

The heart of the integration: the standalone chat loop. This task wires the F4
prefix in `run()` to a new method that owns the editor until the user exits.

**Files:**
- Modify: `ash/auto-shell/src/frontend/repl.rs`:
  - the `Repl` struct definition (line 19-27)
  - a new `\x15` branch in `run()` (insert right after the `\x13` F3 branch, before the `\x05` Ctrl+E branch at line 554)
  - two new methods (`run_chat_loop`, `handle_chat_turn`) — place them near `ask_ai` (after line 394).

- [ ] **Step 1: Add the `chat` field to `Repl`**

In the `Repl` struct (lines 19-27), add the field:

```rust
pub struct Repl {
    shell: Shell,
    line_editor: Reedline,
    prompt: AshPrompt,
    /// Shared completion state — updated after each cd/pushd/etc.
    completion_state: Arc<Mutex<CompletionState>>,
    /// Plan 322: Input mode state (Shell/AutoScript/AI + lock + continuation).
    mode_state: crate::repl_mode::ModeState,
    /// Plan 027: Lazy-initialized persistent AI chat session.
    chat: Option<crate::frontend::ai::ChatSession>,
}
```

Then initialize it in `Repl::new()` — the return statement is at
`repl.rs:272`:

```rust
Ok(Self { shell, line_editor, prompt, completion_state, mode_state: Default::default() })
```

Add `chat: None,` to it:

```rust
Ok(Self { shell, line_editor, prompt, completion_state, mode_state: Default::default(), chat: None })
```

- [ ] **Step 2: Add the `\x15` dispatch branch in `run()`**

In `run()`, immediately after the closing of the `\x13` F3 `if` block (which
ends at line 550 with `continue; }`), and before the `if line.starts_with('\x05')`
Ctrl+E block (line 554), insert:

```rust
                    // Plan 027: F4 = persistent AI chat mode. Enter a
                    // standalone loop that owns the editor until exit.
                    if line.starts_with('\x15') {
                        // Any text typed after F4 is ignored for now
                        // (chat reads full lines in its own loop).
                        self.run_chat_loop()?;
                        continue;
                    }
```

- [ ] **Step 3: Implement `run_chat_loop` and `handle_chat_turn`**

Add these two methods to `impl Repl`, placed right after `ask_ai` (after line
394). The method locks AI mode, lazily loads the session, prints a banner, then
loops reading lines until an exit condition.

```rust
    /// Plan 027: the standalone AI chat loop. Owns the reedline editor until
    /// the user exits via Esc, F4 (toggle-off), F1/F2/F3 (mode switch), or
    /// `/exit`. Persists the conversation on exit.
    fn run_chat_loop(&mut self) -> Result<()> {
        // Lock AI mode so the prompt shows `▌?`.
        self.mode_state.locked = Some(crate::repl_mode::InputMode::AI);
        self.update_prompt();

        // Lazily load the persistent session and print a banner.
        if self.chat.is_none() {
            self.chat = Some(crate::frontend::ai::ChatSession::load());
        }
        let turns = self.chat.as_ref().unwrap().turn_count();
        if turns > 0 {
            println!("  * 已恢复 {} 轮对话 *", turns);
        } else {
            println!("  * 开始新对话 *  (/clear 清空  /exit 退出  F1/F2/F3/Esc 离开)");
        }

        loop {
            let sig = self.line_editor.read_line(&self.prompt);
            let line = match sig {
                Ok(Signal::Success(l)) => l.trim().to_string(),
                Ok(Signal::CtrlD) => break,          // Ctrl-D exits chat
                Ok(Signal::CtrlC) => continue,       // Ctrl-C: new prompt, stay in chat
                Err(_) => continue,
            };

            // Exit prefixes: F4 toggle-off (\x15), Esc (\x14), F1/F2/F3
            // (\x11/\x12/\x13). Save, unlock AI, then hand the prefix back to
            // run() by re-running the same mode-switch logic.
            if let Some(prefix) = line.chars().next() {
                if matches!(prefix, '\x11' | '\x12' | '\x13' | '\x14' | '\x15') {
                    if let Some(session) = self.chat.as_mut() {
                        let _ = session.save();
                    }
                    // Re-dispatch the prefix through the normal mode machinery.
                    match prefix {
                        '\x11' => self.mode_state.toggle_lock(crate::repl_mode::InputMode::Shell),
                        '\x12' => self.mode_state.toggle_lock(crate::repl_mode::InputMode::AutoScript),
                        '\x13' => {
                            // F3 one-shot NL→command: leave chat unlocked; the
                            // outer run() F3 branch handles the actual flow on
                            // the *next* read. For simplicity we just unlock.
                            self.mode_state.locked = None;
                        }
                        // \x14 (Esc) and \x15 (F4): unlock back to shell.
                        _ => self.mode_state.locked = None,
                    }
                    self.update_prompt();
                    // If it was F1/F2/F3 with trailing text, we dropped it
                    // (chat doesn't interpret those). Acceptable for v1.
                    break;
                }
            }

            // Slash commands.
            if let Some(cmd) = crate::frontend::ai::parse_slash_command(&line) {
                match cmd {
                    crate::frontend::ai::SlashCommand::Exit => {
                        if let Some(session) = self.chat.as_mut() {
                            let _ = session.save();
                        }
                        self.mode_state.locked = None;
                        self.update_prompt();
                        break;
                    }
                    crate::frontend::ai::SlashCommand::Clear => {
                        if let Some(session) = self.chat.as_mut() {
                            session.clear();
                            let _ = session.save();
                        }
                        println!("  * 对话已清空 *");
                        continue;
                    }
                }
            }

            // Unknown slash command → no-op notice.
            if line.starts_with('/') {
                println!("  未知命令: {} (可用: /clear /exit)", line);
                continue;
            }

            // Empty line → no-op.
            if line.is_empty() {
                continue;
            }

            // A real chat turn.
            let _ = self.handle_chat_turn(&line);
        }

        Ok(())
    }

    /// Plan 027: send one chat turn using the current cwd. Mirrors `ask_ai`'s
    /// runtime pattern (one-shot current-thread tokio runtime).
    fn handle_chat_turn(&mut self, user: &str) -> Result<()> {
        let system = crate::frontend::ai::build_system_prompt(&self.shell.pwd());
        let session = self.chat.as_mut().expect("chat session initialized in run_chat_loop");
        let result = crate::frontend::ai::block_on_async(
            session.send_turn_streaming(user, &system),
        );
        match result {
            Ok(_full_text) => {
                let _ = session.save();
            }
            Err(e) => {
                eprintln!(
                    "  AI error: {}\n  (set ZHIPU_API_KEY / ANTHROPIC_API_KEY / \
                     OPENAI_API_KEY or start the aaid daemon)",
                    e
                );
            }
        }
        Ok(())
    }
```

> **Why re-dispatch F1/F2/F3 inline rather than `return`-ing the prefix to the
> outer loop:** the outer `run()` loop re-reads a *new* line on each iteration,
> so the prefix that triggered the exit would be lost. Doing the mode switch
> inline (and `break`-ing) is the simplest correct behavior. A minor v1
> limitation: if the user types F1+something, the trailing text is ignored.

- [ ] **Step 4: Build and run the full test suite**

Run: `cargo build -p auto-shell && cargo test -p auto-shell`
Expected: builds clean; all existing tests + the new `frontend::ai` tests pass.

- [ ] **Step 5: Commit**

```bash
git add ash/auto-shell/src/frontend/repl.rs
git commit -m "feat(repl): standalone AI chat loop via F4 (Plan 027)"
```

---

## Task 10: Manual integration verification

There is no automated integration test for the network path; verify by hand.

**Files:** none (verification only).

- [ ] **Step 1: Ensure a daemon is reachable**

Either set an API key env var (`ZHIPU_API_KEY` / `ANTHROPIC_API_KEY` /
`OPENAI_API_KEY`) and let `AiClient::new()` auto-start `aaid`, or start `aaid`
manually:

```bash
# from the auto-ai workspace
cargo run -p auto-ai-daemon
```

- [ ] **Step 2: Build the ash binary**

```bash
cargo build -p auto-shell
```

- [ ] **Step 3: Run ash and verify each flow**

```bash
./target/debug/ash      # or ash.exe on Windows
```

Verify each of these (note the prompt changes):

1. Press **F4** → prompt becomes `▌?` and `* 开始新对话 *` banner appears.
2. Type a question, e.g. `how do I find files larger than 10MB?` and Enter →
   answer streams in live, plain text. Prompt returns to `▌?`.
3. Ask a **follow-up** that depends on the first turn (e.g. `show me that as a one-liner`) → answer reflects the prior context (multi-turn memory works).
4. Press **Ctrl-D** in an empty prompt, or type `/exit` → chat exits, prompt returns to `>` (or the F1/F2 mode if you exited via those).
5. Press **Esc** → exits chat, prompt returns to `>`.
6. Type `/clear` → `* 对话已清空 *`; a follow-up question no longer remembers earlier context.
7. Exit ash (`exit`), restart it, press **F4** again → banner says
   `* 已恢复 N 轮对话 *` and the AI remembers the earlier conversation
   (persistence works).
8. Type `/unknown` → prints `未知命令: /unknown ...`.
9. Press **F3** while in shell mode → the existing NL→command one-shot still
   works unchanged (no regression).
10. With **no** daemon and **no** API key: F4 → enter chat, type a question →
    prints `AI error: ...` with the env-var hint, and **stays** in chat
    (doesn't crash, can `/exit`).

- [ ] **Step 4: If all pass, commit any fixes (likely none) and update status**

If manual testing revealed bugs, fix them and commit with
`fix(ai): <description> (Plan 027)`. Then edit this plan file's header from
`Status: Design + Implementation Plan` to `Status: Done`.

---

## Self-review notes

**Spec coverage:** every spec section maps to a task —
- UX (F4/lock/banner/streaming/slash/exit): Tasks 8, 9, 10.
- Dedicated `ai.rs` module + `ChatSession`: Tasks 1, 6, 7.
- `build_system_prompt` / per-request system: Task 2.
- `block_on_async` (mirrors `ask_ai`): Task 3.
- Slash commands: Task 4.
- JSON history (`history_path`, load/save/clear): Tasks 5, 6.
- Model `tier:mid`, max_tokens 4096, temperature 0.4: Task 7 (inline).
- Error handling (no-daemon hint, corrupt-JSON recovery): Tasks 6, 7, 9.
- Testing strategy (pure fns + tempdir, network is manual): Tasks 2-6 + Task 10.
- F4 keybinding (new, F3 unchanged): Task 8.

**Type/method consistency:** `ChatSession::{load, with_history_path, turn_count, push_user, push_assistant, clear, save, send_turn_streaming}` are defined in Task 6/7 and used in Task 9 with identical names and signatures. `build_system_prompt`, `block_on_async`, `parse_slash_command`, `SlashCommand`, `history_path` are defined in Tasks 2-5 and used unchanged in Task 9. `Message::user`/`Message::assistant` and `CompletionRequest`/`CompletionResponse` match the verified `ai-config` signatures.

**Known v1 limitations (acceptable per spec):** no tool calling, no markdown rendering, fresh `AiClient` per turn, Ctrl+C mid-generation relies on the daemon HTTP timeout, and trailing text after F1/F2/F3 during chat is dropped.

