//! What the app persists between runs, and the errors that reading it can
//! produce.
//!
//! One file, `settings.json`, under `infra::app_dir`. Never any secret: an
//! API key lives sealed in `infra::llm_credentials_store`, keyed by the
//! provider id stored here. The split is what makes `settings.json` safe to
//! read, diff, back up and paste into a bug report.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// One configured LLM provider.
///
/// Unlike its counterpart in Alfa Atlas, every entry here is a complete
/// definition rather than an override of a compiled-in preset — this build
/// ships no presets (see `docs/06-port-plan.md`, F-1.12c), so `base_url` is a
/// plain `String` and a half-filled entry simply does not exist yet.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderConfig {
    pub id: String,
    pub base_url: String,
    /// The model to send. `None` means "whichever the provider lists first",
    /// resolved once and then written back here — see
    /// `services::llm_session::effective_model`.
    #[serde(default)]
    pub model: Option<String>,
    /// A certificate authority to trust *instead of* the public roots, for a
    /// gateway that sits behind a corporate CA. Without this the app is
    /// unusable in exactly the environment it was built for.
    #[serde(default)]
    pub trusted_cert_pem: Option<String>,
    /// Extra headers on every request to this provider — a gateway's own
    /// tracing or routing headers. `$uuid` as a value is replaced per request
    /// (see `infra::llm_providers::openai_compatible`).
    #[serde(default)]
    pub request_headers: HashMap<String, String>,
    /// `None` sends the parameter not at all, which is the only safe default
    /// for an endpoint we know nothing about: some reasoning models reject
    /// any temperature but their own, and a gateway that has never heard of
    /// `reasoning_effort` rejects the whole request rather than ignoring the
    /// field.
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub max_tokens: Option<u32>,
    /// How big this model's context window is, in tokens. `None` means the
    /// app does not know — which is the honest default for a gateway it has
    /// never heard of, and it is why compaction stays off until someone says.
    /// Guessing here would throw away conversation to solve a problem that
    /// may not exist.
    #[serde(default)]
    pub context_limit: Option<u32>,
    /// How hard a reasoning model should think — `"low"`/`"medium"`/`"high"`
    /// as that gateway spells it. A free string on purpose: gateways disagree
    /// on the vocabulary, and a value this app has never heard of must still
    /// be sendable without a rebuild.
    #[serde(default)]
    pub reasoning_effort: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LlmSettings {
    /// Which provider a turn uses. `None` falls back to the first configured
    /// one, so a single-provider setup needs no choice at all.
    pub active_provider_id: Option<String>,
    /// A `Vec`, not a map, so the file stays diffable and the order the user
    /// sees is the order they wrote. Lookup is linear over a handful of
    /// entries.
    pub providers: Vec<ProviderConfig>,
    /// Off by default: a conversation carries the contents of whatever files
    /// the agent read, and that is not something to write to disk unasked.
    /// See `infra::llm_debug_log`.
    pub debug_logging: bool,
}

impl LlmSettings {
    pub fn provider(&self, id: &str) -> Option<&ProviderConfig> {
        self.providers.iter().find(|p| p.id == id)
    }

    /// The provider to use when the caller has no opinion: the active one, or
    /// the only sensible default when nothing is pinned.
    pub fn active(&self) -> Option<&ProviderConfig> {
        match &self.active_provider_id {
            // A pin naming a provider that has since been removed resolves to
            // nothing, rather than silently to some other provider — the key,
            // the model and the endpoint would all be someone else's.
            Some(id) => self.provider(id),
            None => self.providers.first(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppSettings {
    pub llm: LlmSettings,
    pub skills: crate::domain::skills::SkillsSettings,
}

#[derive(Debug, Error)]
pub enum SettingsError {
    #[error("{0}")]
    AppDir(String),
    #[error("failed to read settings: {0}")]
    Read(#[source] std::io::Error),
    #[error("failed to write settings: {0}")]
    Write(String),
    #[error("settings.json is not valid: {0}")]
    Parse(#[source] serde_json::Error),
    #[error("failed to serialize settings: {0}")]
    Serialize(#[source] serde_json::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider(id: &str) -> ProviderConfig {
        ProviderConfig {
            id: id.to_string(),
            base_url: format!("https://{id}.example/v1"),
            ..Default::default()
        }
    }

    #[test]
    fn an_empty_file_is_valid_settings() {
        let settings: AppSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(settings, AppSettings::default());
        assert!(settings.llm.active().is_none());
    }

    #[test]
    fn a_provider_entry_needs_only_an_id_and_a_url() {
        let settings: AppSettings = serde_json::from_str(
            r#"{"llm":{"providers":[{"id":"local","baseUrl":"http://127.0.0.1:1234/v1"}]}}"#,
        )
        .unwrap();
        let active = settings.llm.active().expect("the only provider is active");
        assert_eq!(active.id, "local");
        assert_eq!(active.model, None);
        assert!(active.request_headers.is_empty());
    }

    #[test]
    fn with_nothing_pinned_the_first_provider_is_active() {
        let settings = LlmSettings {
            providers: vec![provider("one"), provider("two")],
            ..Default::default()
        };
        assert_eq!(settings.active().unwrap().id, "one");
    }

    #[test]
    fn a_pin_wins_over_the_order() {
        let settings = LlmSettings {
            active_provider_id: Some("two".to_string()),
            providers: vec![provider("one"), provider("two")],
            ..Default::default()
        };
        assert_eq!(settings.active().unwrap().id, "two");
    }

    /// Falling back to "the first one" here would send the conversation to a
    /// different endpoint, under a different key, without saying so.
    #[test]
    fn a_pin_naming_a_removed_provider_resolves_to_nothing() {
        let settings = LlmSettings {
            active_provider_id: Some("gone".to_string()),
            providers: vec![provider("one")],
            ..Default::default()
        };
        assert!(settings.active().is_none());
    }

    /// The one thing `settings.json` must never contain.
    #[test]
    fn a_provider_entry_has_nowhere_to_put_an_api_key() {
        let written = serde_json::to_string(&ProviderConfig {
            id: "openai".to_string(),
            base_url: "https://api.openai.com/v1".to_string(),
            ..Default::default()
        })
        .unwrap();
        assert!(!written.to_lowercase().contains("key"), "{written}");
    }
}
