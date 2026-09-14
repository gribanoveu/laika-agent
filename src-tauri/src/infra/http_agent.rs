//! The `ureq::Agent` every provider request goes through.
//!
//! TLS trust is configured per agent rather than globally, so a provider that
//! needs its own certificate authority trusted gets it without changing what
//! any other provider trusts.
//!
//! Small module, two hard-won details. Both are in the comments below, because
//! both look like details worth simplifying away and are not.

use thiserror::Error;

#[derive(Debug, Error)]
#[error("tls configuration error: {0}")]
pub struct TlsError(pub String);

/// Builds an agent, optionally trusting a specific certificate authority.
///
/// When `trusted_cert_pem` is `Some`, those certificates **replace** the trust
/// store rather than adding to it. That is the right shape here: a provider
/// either needs its own internal CA trusted or it does not, and every provider
/// has its own agent, so narrowing one cannot affect another.
pub fn build_agent(trusted_cert_pem: Option<&str>) -> Result<ureq::Agent, TlsError> {
    // Turns off ureq's habit of converting a non-2xx status into a bare error
    // *before* the caller can read the response body. That default is what
    // makes a provider's rejection arrive as an undiagnosable "http status:
    // 400" with the explanation still sitting unread in the body.
    let mut builder = ureq::Agent::config_builder().http_status_as_error(false);

    if let Some(pem) = trusted_cert_pem {
        let certs = parse_trusted_certs(pem)?;
        let tls = ureq::tls::TlsConfig::builder()
            .root_certs(ureq::tls::RootCerts::Specific(std::sync::Arc::new(certs)))
            .build();
        builder = builder.tls_config(tls);
    }

    Ok(builder.build().new_agent())
}

/// Parses **every** certificate in `pem`, not just the first.
///
/// `ureq::tls::Certificate::from_pem` is documented to take only the first one
/// it finds. A corporate CA is commonly issued as a chain — a root plus one or
/// more intermediates — and someone pasting that whole chain expects all of it
/// trusted, not silently whichever certificate happens to come first.
pub fn parse_trusted_certs(pem: &str) -> Result<Vec<ureq::tls::Certificate<'static>>, TlsError> {
    let certs = ureq::tls::parse_pem(pem.as_bytes())
        .filter_map(|item| match item {
            Ok(ureq::tls::PemItem::Certificate(cert)) => Some(Ok(cert)),
            Err(e) => Some(Err(e)),
            // `PemItem` is non-exhaustive. Anything that is not a certificate —
            // a private key pasted alongside, say — is not a trust root, and is
            // skipped rather than treated as an error.
            Ok(_) => None,
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| TlsError(e.to_string()))?;

    if certs.is_empty() {
        return Err(TlsError("no PEM-encoded certificate found".to_string()));
    }
    Ok(certs)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Two throwaway self-signed certificates, generated with
    // `openssl req -x509 -newkey rsa:2048 -nodes -days 1`. Real, structurally
    // valid X.509 once decoded, issued by nothing, and never used to connect
    // anywhere — they exist so these tests run against real PEM encoding
    // rather than hand-typed placeholder text.
    const ROOT_1: &str = "-----BEGIN CERTIFICATE-----\n\
MIIDGTCCAgGgAwIBAgIUUwNrcXwZtyrUlfFQy8tHPCVEvWcwDQYJKoZIhvcNAQEL\n\
BQAwHDEaMBgGA1UEAwwRYXRsYXMtdGVzdC1yb290LTEwHhcNMjYwOTE0MTcwODMw\n\
WhcNMjYwOTE1MTcwODMwWjAcMRowGAYDVQQDDBFhdGxhcy10ZXN0LXJvb3QtMTCC\n\
ASIwDQYJKoZIhvcNAQEBBQADggEPADCCAQoCggEBALGcbS8UuU13J7B2MvsVBryI\n\
BerxnuXbc7jJapG7iKCqUmhOV5StsQCrc3Trce/+vFMcc5B/PDa+T32WifSF2Gws\n\
rWNz8Vlb0ZK4ZAHw6NSCssnye798AwpUBTC3SpDoVj5V5dIFtsifFuUqOZ6bPXBE\n\
zIgtDbP7p3dLtS9BR6NMopOU1ArDf6KUu+DPtQ0f4GVfJRXINPUdHx44quFeP+qr\n\
HOEY8NJLoTBdLCcCLgHRFhQ8Rmld6iibb+Z01+UPlhj7b6gXbs40o/9nysGJN9cJ\n\
vmJ29hEk//P1BtMwQd1U+RlKT6yFj+9h1QEMWrKOpBgyzvwTsFGpZdiTpuLytEcC\n\
AwEAAaNTMFEwHQYDVR0OBBYEFIoBVmG+GjtqKIS9FZvflT3fSd6oMB8GA1UdIwQY\n\
MBaAFIoBVmG+GjtqKIS9FZvflT3fSd6oMA8GA1UdEwEB/wQFMAMBAf8wDQYJKoZI\n\
hvcNAQELBQADggEBADYt9JIQbw6vMjZu5neN+qPXmGkFF68G/KSW2AZUeVi/W9iO\n\
Bq87ew+ng2x+LNyji6ofkT5yL97KIjUBSGLwRY+ttpNw8DPXEYv2EMWr7rK/dFNu\n\
1s68XHlnEz2dYwPBX9BKVtJUjNkpRS4OyQVDRzdyHHUr1aoVQuEjyDPlf91zzAy4\n\
vBP1aF0zrqu425xiUONFSuQgOb49/UQiWiiQ6VMJFDZmn7vyPeLpM6gRRdcPu6Lh\n\
W0CNdx/cYtzUTDCSQ4XZueAwyZ0VHIjSPUu/+pY+/wgZhjI2y5fHk8C9ikkWr3ZQ\n\
BySdEVT+BMko44hg1eWw9Ue7lP8cvuuYOOLy2pM=\n\
-----END CERTIFICATE-----\n";

    const ROOT_2: &str = "-----BEGIN CERTIFICATE-----\n\
MIIDGTCCAgGgAwIBAgIUSS5LgvekXENrTftkVhSW3bA16RowDQYJKoZIhvcNAQEL\n\
BQAwHDEaMBgGA1UEAwwRYXRsYXMtdGVzdC1yb290LTIwHhcNMjYwOTE0MTcwODMw\n\
WhcNMjYwOTE1MTcwODMwWjAcMRowGAYDVQQDDBFhdGxhcy10ZXN0LXJvb3QtMjCC\n\
ASIwDQYJKoZIhvcNAQEBBQADggEPADCCAQoCggEBAOA4ilJGiL5oe4peMOUhRzJV\n\
R9ZENMcO6bteiFslHh6qPnJ/hb5a9XkJZfUB+0d2ELlrWNtle2lRYAMNTBfiDQQZ\n\
CzZHt1IvVOLAkNz3ka5Wvnw8p1tqODjbyyIphhF/+OBUPL00YffZlU9/6JY3jfPP\n\
YpL/Rn9zXf3H4Pzz6I+/lpwhcoZYXbBxB0R5KATUpLwCrh/Vry/cPfqcKUgtjNOK\n\
egD+nw5I7lpgTTDzET37vx/RxVscUzAZNVnFdOJq01Upazb7R/qrkV9M8jHth8Bx\n\
+AdAy7Zu/cPYCnMXATZBoa7mKkq8+6AhbyE3PSMtg1vLgxlmdbDwFqRgqan/upMC\n\
AwEAAaNTMFEwHQYDVR0OBBYEFO5HKYUVmrZM94YjTETKwiuMjpz7MB8GA1UdIwQY\n\
MBaAFO5HKYUVmrZM94YjTETKwiuMjpz7MA8GA1UdEwEB/wQFMAMBAf8wDQYJKoZI\n\
hvcNAQELBQADggEBABMT2t16ksy1GDdL2r/2C0uaxB162tIC9SJxA3Z45Kw+CyFQ\n\
bAQJjD9LWzRGHm9npQnLn/yrCaCnrP5NTAgKfCAz3Nqt7niNEvC7HJvDDJ0A/t7A\n\
SYDGztL9G4liHGSWWwYWQz7XGR7NyLOroiFWKgWPDk3nysOlOElbwu5LOEFvdtVt\n\
nSiPnLFb5nds8pLshbdJwcWb2d0aEPrnqUnv+7JXnZeiq//68jv2KyCxVGdEp+d0\n\
BxjqdEz5+eDacnZhml5ezP0isgHv60MK0eAtofLJRS6TABV0vwmO8nAKHGHLPRig\n\
gxvaf8JES4ZQc67yzeks3IKB5uhP+V2w2QD5KrA=\n\
-----END CERTIFICATE-----\n";

    #[test]
    fn no_override_builds_an_agent_on_the_public_roots() {
        assert!(build_agent(None).is_ok());
    }

    #[test]
    fn one_certificate_is_trusted() {
        assert_eq!(parse_trusted_certs(ROOT_1).expect("parses").len(), 1);
        assert!(build_agent(Some(ROOT_1)).is_ok());
    }

    /// The reason this function exists instead of `Certificate::from_pem`. A
    /// corporate CA arrives as a chain, and taking only the first certificate
    /// trusts the root while silently dropping the intermediate that actually
    /// signs the server — which fails at handshake time, far from here.
    #[test]
    fn every_certificate_in_a_chain_is_trusted() {
        let chain = format!("{ROOT_1}{ROOT_2}");
        assert_eq!(
            parse_trusted_certs(&chain).expect("parses").len(),
            2,
            "only part of the chain was trusted"
        );
    }

    /// Surrounding text is ordinary in a pasted chain — `openssl x509 -text`
    /// prints a human-readable block above each certificate.
    #[test]
    fn text_around_the_certificates_is_ignored() {
        let noisy = format!("subject=CN=atlas-test-root-1\n{ROOT_1}trailing notes\n");
        assert_eq!(parse_trusted_certs(&noisy).expect("parses").len(), 1);
    }

    /// Silently trusting nothing would leave the agent on the public roots and
    /// fail at handshake time, with nothing pointing back at the paste.
    #[test]
    fn a_pem_with_no_certificate_is_refused() {
        for input in ["", "not a certificate at all", "-----BEGIN PRIVATE KEY-----\nAAAA\n-----END PRIVATE KEY-----\n"] {
            let err = parse_trusted_certs(input).expect_err("nothing to trust");
            assert!(err.to_string().contains("no PEM-encoded certificate"), "{err}");
        }
    }

    #[test]
    fn damaged_certificate_data_is_refused() {
        let damaged = ROOT_1.replace("MII", "!!!");
        assert!(parse_trusted_certs(&damaged).is_err());
    }
}
