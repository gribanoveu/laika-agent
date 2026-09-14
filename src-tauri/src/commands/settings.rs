//! Configuring the provider a turn talks to.
//!
//! The API key travels in one direction only. It goes in through
//! [`llm_api_key_save`], is sealed, and never comes back out: what the window
//! can learn is whether one exists. Everything else about a provider is
//! ordinary, readable configuration.

use std::sync::Arc;

use serde::Serialize;
use tauri::State;

use crate::domain::settings::ProviderConfig;
use crate::infra::{llm_credentials_store, settings_store};
use crate::services::llm_session;

use super::chat::AgentState;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderView {
    #[serde(flatten)]
    config: ProviderConfig,
    /// Whether a key is stored — never the key itself.
    has_api_key: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmSettingsView {
    providers: Vec<ProviderView>,
    active_provider_id: Option<String>,
    debug_logging: bool,
}

#[tauri::command]
pub fn llm_settings_get() -> Result<LlmSettingsView, String> {
    let settings = settings_store::load().map_err(|e| e.to_string())?.llm;
    Ok(LlmSettingsView {
        providers: settings
            .providers
            .into_iter()
            .map(|config| ProviderView {
                has_api_key: llm_credentials_store::has_api_key(&config.id),
                config,
            })
            .collect(),
        active_provider_id: settings.active_provider_id,
        debug_logging: settings.debug_logging,
    })
}

#[tauri::command]
pub fn llm_provider_save(provider: ProviderConfig) -> Result<(), String> {
    if provider.id.trim().is_empty() {
        return Err("a provider needs a name".to_string());
    }
    if provider.base_url.trim().is_empty() {
        return Err("a provider needs a base URL".to_string());
    }
    llm_session::save_provider(provider).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn llm_provider_remove(id: String) -> Result<(), String> {
    llm_session::remove_provider(&id).map_err(|e| e.to_string())
}

/// Seals the key under the app master key. There is no command that reads one
/// back, which is the point.
#[tauri::command]
pub fn llm_api_key_save(id: String, key: String) -> Result<(), String> {
    if key.trim().is_empty() {
        return llm_credentials_store::delete_api_key(&id);
    }
    llm_credentials_store::save_api_key(&id, key.trim())
}

#[tauri::command]
pub fn llm_active_provider_set(id: Option<String>) -> Result<(), String> {
    llm_session::set_active_provider(id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn llm_debug_logging_set(enabled: bool) -> Result<(), String> {
    llm_session::set_debug_logging(enabled).map_err(|e| e.to_string())
}

/// Asks the provider what it serves. A live call, so it is also the one thing
/// that proves the base URL and the key are both right.
#[tauri::command]
pub async fn llm_models_list(id: Option<String>) -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let session = llm_session::resolve(id.as_deref()).map_err(|e| e.to_string())?;
        session
            .provider
            .list_models()
            .map(|models| models.into_iter().map(|m| m.id).collect())
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("the request thread failed: {e}"))?
}

/// Whether a turn could start right now, and if not, what is missing. Asked
/// before sending rather than discovered as a failed turn.
#[tauri::command]
pub fn agent_readiness(state: State<'_, Arc<AgentState>>) -> Readiness {
    let workspace = super::chat::workspace_current(state);
    let provider = settings_store::load()
        .ok()
        .and_then(|settings| settings.llm.active().cloned());
    Readiness {
        has_key: provider
            .as_ref()
            .map(|p| llm_credentials_store::has_api_key(&p.id))
            .unwrap_or(false),
        provider: provider.map(|p| p.id),
        workspace,
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Readiness {
    pub workspace: Option<String>,
    pub provider: Option<String>,
    pub has_key: bool,
}
