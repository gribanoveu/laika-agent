//! The master key every sealed secret is encrypted under.
//!
//! The authoritative store is the OS keychain — macOS Keychain, Windows
//! Credential Manager, Linux Secret Service. The file beside the sealed blobs
//! is a fallback for machines where no keychain is reachable.
//!
//! Why the keychain is worth the trouble: the file fallback keeps the key in
//! plaintext next to the ciphertext it unlocks, so anything that can copy the
//! app directory — a backup, a synced home directory, "send me your config" —
//! gets both halves. Moving the key into the keychain removes it from every
//! file-copy route. It does **not** defend against malware running as the same
//! user: on all three platforms an unlocked keychain is readable by that user's
//! processes. That tier needs a master password, which is a different feature.
//!
//! ## A store that only pretends to work
//!
//! `keyring` 3.x makes its platform backends opt-in features; with none
//! enabled it compiles to an in-memory mock whose writes return `Ok(())` and
//! evaporate at process exit. A missing Secret Service on Linux fails less
//! quietly but just as fatally. So a successful write is never taken as proof:
//! the probe writes a marker and reads it back through a *separate* entry,
//! which the mock cannot satisfy because it keeps state per entry.
//!
//! ## Never minting a second key
//!
//! "The keychain holds no key" and "the keychain could not answer" must never
//! be conflated. Only the first makes it safe to generate one. The second
//! happens routinely — on macOS a keychain item's ACL names the binary that
//! created it, so every rebuilt development binary is refused on read while
//! still free to create its own probe item. Minting a replacement there would
//! leave every sealed secret permanently unreadable, so a recorded marker
//! saying a key exists turns an unanswerable read into an error rather than a
//! fresh start.

use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use aes_gcm::aead::OsRng;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

pub const KEY_LEN: usize = 32;
pub type MasterKey = Zeroizing<[u8; KEY_LEN]>;

const KEYRING_SERVICE: &str = "com.eugene.atlas-desktop";
const KEYRING_USER: &str = "master-key";
/// Used only by the probe, never for real key material.
const KEYRING_PROBE_USER: &str = "backend-probe";
const KEY_FILE: &str = "master.key";
/// Records *where* the key went, so a later run can tell "no key yet" from
/// "the key is somewhere I cannot read right now". Holds no key material.
const MARKER_FILE: &str = "master_key_store.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum KeyStore {
    Keychain,
    File,
}

#[derive(Serialize, Deserialize)]
struct Marker {
    store: KeyStore,
}

static CACHE: Mutex<Option<[u8; KEY_LEN]>> = Mutex::new(None);

/// The master key, generating one on first use.
pub fn resolve() -> Result<MasterKey, String> {
    let mut cache = CACHE.lock().map_err(|_| "key cache is poisoned".to_string())?;
    if let Some(key) = *cache {
        return Ok(Zeroizing::new(key));
    }
    let key = load_or_create()?;
    *cache = Some(*key);
    Ok(key)
}

fn load_or_create() -> Result<MasterKey, String> {
    if backend_usable() {
        match keychain_get() {
            Ok(Some(key)) => {
                record(KeyStore::Keychain);
                return Ok(key);
            }
            Ok(None) => {}
            // Could not answer. See the module comment: minting a replacement
            // here is what destroys every sealed secret.
            Err(e) => {
                if recorded() == Some(KeyStore::Keychain) {
                    return Err(format!(
                        "the master key is in the keychain but could not be read ({e}). \
                         Refusing to generate a new one, which would make every stored \
                         secret unreadable."
                    ));
                }
            }
        }
    }

    if let Some(key) = file_read()? {
        record(KeyStore::File);
        return Ok(key);
    }

    let mut fresh = [0u8; KEY_LEN];
    OsRng.fill_bytes(&mut fresh);
    let key = Zeroizing::new(fresh);

    if backend_usable() && keychain_put(&key).is_ok() {
        record(KeyStore::Keychain);
    } else {
        file_write(&key)?;
        record(KeyStore::File);
    }
    Ok(key)
}

/// Whether the keychain is real, rather than a store that accepts writes and
/// forgets them.
fn backend_usable() -> bool {
    let mut marker = [0u8; 8];
    OsRng.fill_bytes(&mut marker);

    if backend::set(KEYRING_PROBE_USER, &marker).is_err() {
        return false;
    }
    // A separate read, not a handle kept from the write: the mock keeps the
    // value inside the entry that wrote it, so only a real backend can answer.
    let usable = matches!(backend::get(KEYRING_PROBE_USER), Ok(Some(v)) if v == marker);
    let _ = backend::delete(KEYRING_PROBE_USER);
    usable
}

fn keychain_get() -> Result<Option<MasterKey>, String> {
    match backend::get(KEYRING_USER)? {
        Some(bytes) if bytes.len() == KEY_LEN => {
            let mut key = [0u8; KEY_LEN];
            key.copy_from_slice(&bytes);
            Ok(Some(Zeroizing::new(key)))
        }
        // Present but the wrong size is corruption, not absence — treating it
        // as absence would mint a second key over the top of the first.
        Some(_) => Err("the stored master key is not the right size".to_string()),
        None => Ok(None),
    }
}

fn keychain_put(key: &[u8; KEY_LEN]) -> Result<(), String> {
    backend::set(KEYRING_USER, key)
}

fn file_read() -> Result<Option<MasterKey>, String> {
    let path = app_dir()?.join(KEY_FILE);
    match fs::read(&path) {
        Ok(bytes) if bytes.len() == KEY_LEN => {
            let mut key = [0u8; KEY_LEN];
            key.copy_from_slice(&bytes);
            Ok(Some(Zeroizing::new(key)))
        }
        Ok(_) => Err(format!("{} is not a valid master key", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("could not read the master key: {e}")),
    }
}

fn file_write(key: &[u8; KEY_LEN]) -> Result<(), String> {
    let dir = app_dir()?;
    fs::create_dir_all(&dir).map_err(|e| format!("could not create {}: {e}", dir.display()))?;
    let path = dir.join(KEY_FILE);
    fs::write(&path, key).map_err(|e| format!("could not write the master key: {e}"))?;

    // Owner-only. On a shared machine the fallback is otherwise readable by
    // every other account on it.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

fn recorded() -> Option<KeyStore> {
    let path = app_dir().ok()?.join(MARKER_FILE);
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str::<Marker>(&text).ok().map(|m| m.store)
}

/// Written after every successful resolution, so the record follows reality
/// rather than intent.
fn record(store: KeyStore) {
    let Ok(dir) = app_dir() else { return };
    if fs::create_dir_all(&dir).is_err() {
        return;
    }
    if let Ok(text) = serde_json::to_string(&Marker { store }) {
        let _ = fs::write(dir.join(MARKER_FILE), text);
    }
}

#[cfg(not(test))]
fn app_dir() -> Result<PathBuf, String> {
    dirs::home_dir()
        .map(|home| home.join(".atlas-desktop"))
        .ok_or_else(|| "no home directory".to_string())
}

#[cfg(test)]
fn app_dir() -> Result<PathBuf, String> {
    tests::app_dir_override()
}

/// The keychain, or an in-process double under test.
///
/// The suite must never touch a real keychain: it would prompt on macOS, fail
/// in CI, and — worst — could overwrite the developer's own key and leave every
/// secret they have stored undecryptable.
#[cfg(not(test))]
mod backend {
    use super::{KEYRING_SERVICE, KEYRING_USER};

    fn entry(user: &str) -> Result<keyring::Entry, String> {
        keyring::Entry::new(KEYRING_SERVICE, user).map_err(|e| e.to_string())
    }

    pub fn get(user: &str) -> Result<Option<Vec<u8>>, String> {
        match entry(user)?.get_secret() {
            Ok(bytes) => Ok(Some(bytes)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }

    pub fn set(user: &str, value: &[u8]) -> Result<(), String> {
        entry(user)?.set_secret(value).map_err(|e| e.to_string())
    }

    pub fn delete(user: &str) -> Result<(), String> {
        match entry(user)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }

    // Referenced so the constant is not dead in this build.
    #[allow(dead_code)]
    fn _key_user() -> &'static str {
        KEYRING_USER
    }
}

#[cfg(test)]
use tests::backend;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::temp_dir;
    use std::collections::HashMap;
    use std::sync::MutexGuard;

    /// How the doubled keychain behaves for the test in progress.
    #[derive(Clone, Copy, PartialEq, Eq)]
    pub(super) enum Mode {
        /// A real backend: values persist and any entry can read them.
        Working,
        /// `keyring`'s mock: writes succeed and are kept per entry, so reading
        /// through a different entry finds nothing.
        Mock,
        /// Working, except that the *existing* key entry is refused.
        ///
        /// This is the macOS ACL case, and the shape matters: a keychain item's
        /// ACL names the binary that created it, so a rebuilt binary is refused
        /// on that item while still free to create and read its own new ones.
        /// The probe therefore passes and says nothing at all about the item
        /// that matters — which is exactly why the marker exists.
        Unreadable,
    }

    struct TestState {
        dir: PathBuf,
        mode: Mode,
        store: HashMap<String, Vec<u8>>,
    }

    static STATE: Mutex<Option<TestState>> = Mutex::new(None);
    /// These tests share the key cache, the doubled keychain and the app
    /// directory, so they run one at a time.
    static SERIAL: Mutex<()> = Mutex::new(());

    pub(crate) mod backend {
        use super::STATE;

        pub fn get(user: &str) -> Result<Option<Vec<u8>>, String> {
            let guard = STATE.lock().expect("state");
            let state = guard.as_ref().expect("test state is installed");
            match state.mode {
                // Only the key itself is refused; the probe reads fine.
                super::Mode::Unreadable if user == super::KEYRING_USER => {
                    Err("access denied by the keychain".into())
                }
                // The mock keeps a write inside the entry that made it, so a
                // separate read never sees it.
                super::Mode::Mock => Ok(None),
                _ => Ok(state.store.get(user).cloned()),
            }
        }

        pub fn set(user: &str, value: &[u8]) -> Result<(), String> {
            let mut guard = STATE.lock().expect("state");
            let state = guard.as_mut().expect("test state is installed");
            state.store.insert(user.to_string(), value.to_vec());
            Ok(())
        }

        pub fn delete(user: &str) -> Result<(), String> {
            let mut guard = STATE.lock().expect("state");
            guard.as_mut().expect("state").store.remove(user);
            Ok(())
        }
    }

    pub(super) fn app_dir_override() -> Result<PathBuf, String> {
        Ok(STATE.lock().expect("state").as_ref().expect("state").dir.clone())
    }

    /// Installs a fresh app directory and doubled keychain, and clears the
    /// cached key. Returns the serialising guard.
    fn setup(label: &str, mode: Mode) -> MutexGuard<'static, ()> {
        let guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        *STATE.lock().expect("state") = Some(TestState {
            dir: temp_dir(label),
            mode,
            store: HashMap::new(),
        });
        *CACHE.lock().expect("cache") = None;
        guard
    }

    fn set_mode(mode: Mode) {
        STATE.lock().expect("state").as_mut().expect("state").mode = mode;
        *CACHE.lock().expect("cache") = None;
    }

    #[test]
    fn a_working_keychain_holds_the_key() {
        let _serial = setup("mk-keychain", Mode::Working);

        let first = resolve().expect("mints a key");
        *CACHE.lock().unwrap() = None;
        let second = resolve().expect("finds the same key again");

        assert_eq!(*first, *second);
        assert_eq!(recorded(), Some(KeyStore::Keychain));
        assert!(
            !app_dir_override().unwrap().join(KEY_FILE).exists(),
            "a usable keychain must not leave the key on disk"
        );
    }

    /// The trap the probe exists for: a store whose writes return Ok(()) and
    /// evaporate. Without the probe the key would be "saved" every run and
    /// lost every time.
    #[test]
    fn a_mock_keychain_is_not_trusted() {
        let _serial = setup("mk-mock", Mode::Mock);

        let first = resolve().expect("mints a key");
        *CACHE.lock().unwrap() = None;
        let second = resolve().expect("reads it back");

        assert_eq!(*first, *second, "the key survived, so it went to the file");
        assert_eq!(recorded(), Some(KeyStore::File));
        assert!(app_dir_override().unwrap().join(KEY_FILE).exists());
    }

    /// The data-loss guard. The key is in the keychain, the keychain refuses to
    /// answer, and minting a replacement would leave every sealed secret
    /// unreadable — so this fails loudly instead.
    #[test]
    fn an_unreadable_keychain_refuses_to_mint_a_second_key() {
        let _serial = setup("mk-unreadable", Mode::Working);
        let original = resolve().expect("mints a key");
        assert_eq!(recorded(), Some(KeyStore::Keychain));

        set_mode(Mode::Unreadable);
        let err = resolve().expect_err("must not invent a new key");

        assert!(err.contains("Refusing to generate a new one"), "{err}");
        set_mode(Mode::Working);
        assert_eq!(*resolve().expect("recovers"), *original);
    }

    /// Without a marker saying otherwise, an unanswerable keychain is just a
    /// machine without a usable one — falling back to the file is correct.
    #[test]
    fn an_unreadable_keychain_with_no_recorded_key_falls_back() {
        let _serial = setup("mk-unreadable-fresh", Mode::Mock);

        let key = resolve().expect("falls back to the file");

        assert_eq!(recorded(), Some(KeyStore::File));
        *CACHE.lock().unwrap() = None;
        assert_eq!(*resolve().expect("reads it back"), *key);
    }

    #[test]
    fn the_fallback_file_is_owner_only() {
        let _serial = setup("mk-perms", Mode::Mock);
        resolve().expect("writes the fallback");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let path = app_dir_override().unwrap().join(KEY_FILE);
            let mode = fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600, "the fallback key is readable by others");
        }
    }

    /// A truncated or overwritten file is corruption, and treating it as
    /// absence would quietly mint a second key over the first.
    #[test]
    fn a_damaged_fallback_file_is_an_error_not_a_fresh_start() {
        let _serial = setup("mk-damaged", Mode::Mock);
        resolve().expect("writes the fallback");
        let path = app_dir_override().unwrap().join(KEY_FILE);
        fs::write(&path, b"too short").expect("writable");
        *CACHE.lock().unwrap() = None;

        assert!(resolve().is_err(), "a damaged key file must not be ignored");
    }

    #[test]
    fn the_key_is_the_right_size_and_not_all_zeroes() {
        let _serial = setup("mk-quality", Mode::Working);
        let key = resolve().expect("mints a key");
        assert_eq!(key.len(), KEY_LEN);
        assert!(key.iter().any(|b| *b != 0));
    }
}
