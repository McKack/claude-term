//! The seam between a project's session and the engine behind it: today the Claude CLI (`Conn`, stream-json over
//! stdio), later OpenCode (`opencode serve`, HTTP + SSE, #599). Only what turn.rs really does is in it.
//!
//! Clauracle → engine (this trait): send a turn, interrupt it.
//!
//! Engine → Clauracle (the contract a new backend translates into, all of it already handled by the CLI path):
//! - at start: `{commands, models, agents}` for the composer (turn.rs emits `commands`);
//! - events: stream-json shaped messages (`system`/`init` with session_id, `assistant`, `user` tool results,
//!   `result` ending the turn) fed to `turn::show`, which drives the chat log, usage and the end of a turn;
//! - permission asks: `control::permission` decides (allow rules, auto mode, the user's card);
//! - Clauracle's own tools (ask, ready, ui, restart): `control::mcp`.
//! Starting and stopping the engine stays in `turn::connect_and_serve`, one branch per backend.
use serde_json::{json, Value};
use std::future::Future;

pub trait Backend: Send + Sync {
    /// One user turn: plain text, or content blocks (text plus pasted images).
    fn send(&self, content: Value) -> impl Future<Output = Result<(), String>> + Send;
    /// Stop the running turn (background agents keep going).
    fn interrupt(&self) -> impl Future<Output = Result<(), String>> + Send;
}

impl Backend for super::Conn {
    async fn send(&self, content: Value) -> Result<(), String> {
        self.write(&json!({"type": "user", "message": {"role": "user", "content": content}, "parent_tool_use_id": null, "session_id": "default"})).await
    }
    async fn interrupt(&self) -> Result<(), String> {
        self.request(json!({"subtype": "interrupt"}), 10).await.map(|_| ())
    }
}
