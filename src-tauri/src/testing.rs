//! Helpers shared by unit tests across modules. Compiled only under `cfg(test)`.

use std::path::PathBuf;

/// A fresh directory under the system temp dir, unique per call.
///
/// No `tempfile` dependency and no cleanup guard: a test that wants the
/// directory gone removes it, and one that fails leaves it behind on purpose —
/// on a filesystem test that is usually the only evidence of what went wrong.
pub fn temp_dir(label: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);

    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock is after the epoch")
        .as_nanos();
    let n = N.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("kibo-{label}-{nanos}-{n}"));
    std::fs::create_dir_all(&dir).expect("temp dir is creatable");
    dir
}

/// Runs `f` against a throwaway app directory with a working keychain, holding
/// the app-directory lock for the whole call.
///
/// Everything under `infra::app_dir` — settings, sealed credentials, logs — is
/// process-global state, so tests that touch it cannot run beside each other.
/// One helper rather than a copy per store, for the reason given on
/// `app_dir::test_support`.
pub fn with_app_dir<T>(label: &str, f: impl FnOnce() -> T) -> T {
    let _guard = crate::infra::app_dir::test_support::lock();
    crate::infra::app_dir::test_support::install(temp_dir(label));
    crate::infra::master_key::install_working_keychain_for_tests();
    f()
}
