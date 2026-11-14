//! The agent panel's Tauri commands: thin wrappers of `AgentHost`. The ones
//! that wait for the agent run on a blocking thread.

use super::{adapters, AgentError, AgentHost};
use serde_json::Value;
use std::sync::Arc;
use tauri::State;

type H<'a> = State<'a, Arc<AgentHost>>;
type R<T> = Result<T, AgentError>;

async fn blocking<T: Send + 'static>(
    host: &Arc<AgentHost>,
    f: impl FnOnce(&AgentHost) -> R<T> + Send + 'static,
) -> R<T> {
    let host = host.clone();
    tauri::async_runtime::spawn_blocking(move || f(&host))
        .await
        .map_err(|e| AgentError::msg(e.to_string()))?
}

#[tauri::command]
pub fn agent_adapters(custom: Option<String>) -> Vec<adapters::Adapter> {
    adapters::list(custom.as_deref())
}

#[tauri::command]
pub fn agent_setup(host: H<'_>) -> Value {
    host.setup()
}

#[tauri::command]
pub async fn agent_connect(host: H<'_>, adapter: String, command: Option<String>) -> R<Value> {
    blocking(&host, move |h| h.connect(&adapter, command.as_deref())).await
}

#[tauri::command]
pub async fn agent_disconnect(host: H<'_>) -> R<()> {
    blocking(&host, |h| {
        h.disconnect();
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn agent_session_new(host: H<'_>) -> R<Value> {
    blocking(&host, |h| h.session_new()).await
}

#[tauri::command]
pub async fn agent_session_load(host: H<'_>, session: String) -> R<Value> {
    blocking(&host, move |h| h.session_load(&session)).await
}

#[tauri::command]
pub async fn agent_session_list(host: H<'_>) -> R<Value> {
    blocking(&host, |h| h.session_list()).await
}

#[tauri::command]
pub async fn agent_prompt(host: H<'_>, session: String, prompt: Value) -> R<Value> {
    blocking(&host, move |h| h.prompt(&session, prompt)).await
}

#[tauri::command]
pub async fn agent_cancel(host: H<'_>, session: String) -> R<()> {
    blocking(&host, move |h| h.cancel(&session)).await
}

#[tauri::command]
pub async fn agent_permission(host: H<'_>, key: String, option: Option<String>) -> R<()> {
    blocking(&host, move |h| h.permission(&key, option.as_deref())).await
}

#[tauri::command]
pub async fn agent_set_mode(host: H<'_>, session: String, mode: String) -> R<Value> {
    blocking(&host, move |h| h.set_mode(&session, &mode)).await
}

#[tauri::command]
pub async fn agent_set_model(host: H<'_>, session: String, model: String) -> R<Value> {
    blocking(&host, move |h| h.set_model(&session, &model)).await
}

#[tauri::command]
pub async fn agent_set_config(
    host: H<'_>,
    session: String,
    config: String,
    value: Value,
) -> R<Value> {
    blocking(&host, move |h| h.set_config(&session, &config, value)).await
}

#[tauri::command]
pub async fn agent_authenticate(host: H<'_>, method: String) -> R<Value> {
    blocking(&host, move |h| h.authenticate(&method)).await
}

/// The command line of a terminal login method, and a terminal emulator started on it when `open`.
#[tauri::command]
pub async fn agent_login(host: H<'_>, method: String, open: bool) -> R<Value> {
    blocking(&host, move |h| {
        let cmd = h.login_command(&method)?;
        let terminal = if open {
            Some(adapters::open_terminal(&cmd).map_err(AgentError::msg)?)
        } else {
            None
        };
        Ok(serde_json::json!({ "command": cmd, "terminal": terminal }))
    })
    .await
}

#[tauri::command]
pub async fn agent_files(host: H<'_>) -> R<Vec<String>> {
    blocking(&host, |h| Ok(h.files())).await
}
