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
    let dir = std::env::temp_dir().join(format!("atlas-{label}-{nanos}-{n}"));
    std::fs::create_dir_all(&dir).expect("temp dir is creatable");
    dir
}
