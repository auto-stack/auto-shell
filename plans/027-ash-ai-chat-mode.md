# 027 — ash AI Chat Mode (`?` mode)

**Status:** Design
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
| `Cargo.toml` | **No change** — `auto-ai-client`, `tokio`, `serde_json` already present. |

Net: **1 new file + 2 small edits.** Nothing in v1 is rewritten or deleted;
`ask_ai` and the existing F3 flow are untouched.

## Future path (explicitly out of scope, but enabled)

Because `ChatSession` is an isolated boundary, adding tools/agent later means:
add `auto-ai-agent` to `Cargo.toml`, swap `ChatSession` for an `Agent` inside
`ai.rs` (register shell `Tool` impls), and nothing in `repl.rs` changes.
Likewise, a markdown renderer can later be layered into `send_turn_streaming`'s
delta handler without touching the REPL.
