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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::with_app_dir;

    fn provider(id: &str) -> ProviderConfig {
        ProviderConfig {
            id: id.to_string(),
            base_url: "https://gateway.example/v1".to_string(),
            ..Default::default()
        }
    }

    /// The whole path the settings window drives, in order: configure a
    /// provider, give it a key, ask what is configured. Each piece is tested
    /// where it lives; what this adds is that the three commands agree —
    /// which is the part a window can be wrong about and a unit test cannot.
    #[test]
    fn a_saved_key_is_reported_as_stored() {
        with_app_dir("cmd-settings-key", || {
            llm_provider_save(provider("gateway")).unwrap();
            llm_api_key_save("gateway".to_string(), "sk-live".to_string()).unwrap();
            llm_active_provider_set(Some("gateway".to_string())).unwrap();

            let view = llm_settings_get().unwrap();
            assert_eq!(view.active_provider_id.as_deref(), Some("gateway"));
            assert_eq!(view.providers.len(), 1);
            assert!(view.providers[0].has_api_key, "the key did not survive");

            // And it is the key itself that survived, not merely a flag.
            let stored = llm_credentials_store::get_api_key("gateway").expect("stored");
            assert_eq!(secrecy::ExposeSecret::expose_secret(&stored), "sk-live");
        });
    }

    /// The window sends the whole provider back, including fields it added
    /// itself. A refusal here would take the key with it: the key is saved
    /// after the provider, in the same click.
    #[test]
    fn the_shape_the_window_sends_is_accepted() {
        with_app_dir("cmd-settings-wire", || {
            let wire = serde_json::json!({
                "id": "gateway",
                "baseUrl": "https://gateway.example/v1",
                "model": null,
                "hasApiKey": true,
            });
            let parsed: ProviderConfig = serde_json::from_value(wire).expect("accepted");

            llm_provider_save(parsed).unwrap();
            assert_eq!(llm_settings_get().unwrap().providers.len(), 1);
        });
    }

    /// An empty box means "delete the stored key" — and it must not leave the
    /// provider believing it still has one.
    #[test]
    fn an_emptied_key_box_removes_the_key() {
        with_app_dir("cmd-settings-clear", || {
            llm_provider_save(provider("gateway")).unwrap();
            llm_api_key_save("gateway".to_string(), "sk-live".to_string()).unwrap();
            llm_api_key_save("gateway".to_string(), "  ".to_string()).unwrap();

            assert!(!llm_settings_get().unwrap().providers[0].has_api_key);
        });
    }

    #[test]
    fn a_provider_without_a_name_or_a_url_is_refused() {
        with_app_dir("cmd-settings-blank", || {
            assert!(llm_provider_save(provider("  ")).is_err());
            assert!(llm_provider_save(ProviderConfig {
                id: "gateway".to_string(),
                base_url: " ".to_string(),
                ..Default::default()
            })
            .is_err());
        });
    }
}
