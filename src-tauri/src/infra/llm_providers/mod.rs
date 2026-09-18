pub mod anthropic;
pub mod openai_compatible;

use secrecy::SecretString;

use crate::domain::llm::{LlmError, LlmProvider};
use crate::domain::settings::{ProviderConfig, ProviderKind};
use crate::infra::http_agent;

/// The one place a configuration becomes a client. Callers work against the
/// trait afterwards, never against `OpenAiCompatibleProvider` directly — which
/// is what keeps a second protocol (Anthropic's own, say) to a branch here
/// rather than a change at every call site.
pub fn provider_for(
    config: &ProviderConfig,
    api_key: Option<SecretString>,
) -> Result<Box<dyn LlmProvider>, LlmError> {
    let api_key = api_key.ok_or_else(|| {
        LlmError::Message(format!("no API key is stored for provider \"{}\"", config.id))
    })?;
    let agent = http_agent::build_agent(config.trusted_cert_pem.as_deref())
        .map_err(|e| LlmError::Tls(e.0))?;
    Ok(match config.kind {
        ProviderKind::OpenAiCompatible => Box::new(openai_compatible::OpenAiCompatibleProvider::new(
            agent,
            config.base_url.clone(),
            api_key,
            config.request_headers.clone(),
            config.temperature,
            config.max_tokens,
            config.reasoning_effort.clone(),
        )),
        ProviderKind::Anthropic => Box::new(anthropic::AnthropicProvider::new(
            agent,
            config.base_url.clone(),
            api_key,
            config.request_headers.clone(),
            config.temperature,
            config.max_tokens,
            config.reasoning_effort.clone(),
        )),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(trusted_cert_pem: Option<&str>) -> ProviderConfig {
        ProviderConfig {
            id: "local".to_string(),
            base_url: "https://example.internal/v1".to_string(),
            trusted_cert_pem: trusted_cert_pem.map(|s| s.to_string()),
            ..Default::default()
        }
    }

    /// The most common way a provider is "configured but broken", and the
    /// message has to say which of the two halves is missing.
    #[test]
    fn a_provider_without_a_key_says_so() {
        let Err(err) = provider_for(&config(None), None) else {
            panic!("expected an error");
        };
        assert!(err.to_string().contains("local"), "{err}");
    }

    #[test]
    fn a_damaged_trust_certificate_is_a_tls_error_not_a_key_error() {
        let Err(err) = provider_for(&config(Some("not a pem")), Some(SecretString::from("k")))
        else {
            panic!("expected an error")
        };
        assert!(matches!(err, LlmError::Tls(_)), "{err}");
    }

    #[test]
    fn the_public_trust_store_needs_no_certificate() {
        assert!(provider_for(&config(None), Some(SecretString::from("k"))).is_ok());
    }
}
