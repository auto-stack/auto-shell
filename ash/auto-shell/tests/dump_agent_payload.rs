//! PLAN-083 T-07 e2e payload builder (designs/040 §4): builds the exact
//! ChatSession request (Assistant role + context block + full command-tool
//! set with real schemas + preloaded history + default thinking policy) via
//! a capturing mock client, asserts the request shape (thinking off, ≥75
//! tools), and dumps it as JSON for daemon timing probes (AC-01/AC-02).
//! Run: cargo test -p auto-shell --test dump_agent_payload

use std::sync::{Arc, Mutex};

use auto_ai_agent::{Client as AgentClient, StreamEvent};
use auto_ai_client::{CompletionRequest, CompletionResponse, ClientError};

use auto_shell::ai::ChatSession;

struct CaptureClient {
    captured: Mutex<Vec<CompletionRequest>>,
}

#[async_trait::async_trait]
impl AgentClient for CaptureClient {
    async fn complete(
        &self,
        req: &CompletionRequest,
    ) -> Result<CompletionResponse, ClientError> {
        self.captured.lock().unwrap().push(req.clone());
        Err(ClientError::DaemonUnavailable)
    }
}

#[test]
fn dump_request_payload() {
    // Real-shape history: 15 Q/A text turns (~30 messages, like the
    // 2026-09-24 repro's 37-message history).
    let history_path = std::env::temp_dir().join("ash-083-payload-history.json");
    let mut turns = Vec::new();
    for i in 0..15 {
        turns.push(auto_ai_client::Message::user(&format!(
            "问题{i}:帮我看下 src 目录里最近的改动,并解释这段日志 {}",
            "上下文填充".repeat(20)
        )));
        turns.push(auto_ai_client::Message::assistant(&format!(
            "回答{i}:主要改动是 X 和 Y,日志显示正常 {}",
            "补充说明".repeat(20)
        )));
    }
    std::fs::write(&history_path, serde_json::to_string(&turns).unwrap()).unwrap();

    let cap = Arc::new(CaptureClient {
        captured: Mutex::new(Vec::new()),
    });
    let client: Arc<dyn AgentClient> = cap.clone();
    let mut session = ChatSession::with_client_and_path(client, history_path);

    // The REPL refreshes the context block (cwd/last-command) before each
    // turn. Shell::new() must stay off the tokio runtime — this test is sync,
    // so build it directly.
    let shell = auto_shell::shell::Shell::new();
    session.set_context_str(auto_shell::ai::context::build_context_block(&shell));

    let on_event: Arc<dyn Fn(StreamEvent) + Send + Sync> = Arc::new(|_| {});
    auto_shell::ai::block_on_async(session.send_turn_streaming(
        "你能够帮我统计一下这个目录下的各个子目录占用了多少磁盘空间吗？",
        on_event,
    ));

    let captured = cap.captured.lock().unwrap();
    assert_eq!(captured.len(), 1, "one request captured");
    let req = &captured[0];
    assert_eq!(req.thinking_level.as_deref(), Some("off"), "T-02 default");
    assert_eq!(req.model, "tier:mid");
    assert!(req.tools.len() >= 75, "full command tool set, got {}", req.tools.len());
    let out = std::env::temp_dir().join("req_ash083.json");
    std::fs::write(&out, serde_json::to_string_pretty(req).unwrap()).unwrap();
    println!(
        "dumped {} bytes to {}",
        std::fs::metadata(&out).unwrap().len(),
        out.display()
    );
}
