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
