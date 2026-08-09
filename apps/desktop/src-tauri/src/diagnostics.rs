use crate::database::Database;
use crate::errors::{AppError, AppResult};
use crate::models::{DiagnosticClearResult, DiagnosticRetentionStatus};
use chrono::{DateTime, Duration, Utc};
use ring::aead::{AES_256_GCM, Aad, LessSafeKey, Nonce, UnboundKey};
use ring::rand::{SecureRandom, SystemRandom};
use std::sync::{Arc, Mutex};

const RETENTION_DAYS: i64 = 7;
const KEYCHAIN_SERVICE: &str = "com.vibemeter.desktop.diagnostic-retention";
const KEYCHAIN_ACCOUNT: &str = "raw-live-envelope-key-v1";
const ENVELOPE_AAD: &[u8] = b"vibemeter-diagnostic-envelope-v1";

trait SecureKeyStore: Send + Sync {
    fn load(&self) -> AppResult<Option<Vec<u8>>>;
    fn store(&self, key: &[u8]) -> AppResult<()>;
    fn delete(&self) -> AppResult<()>;
}

#[derive(Clone)]
pub struct DiagnosticRetention {
    database: Database,
    storage_location: Arc<str>,
    secure_store: Arc<dyn SecureKeyStore>,
    operations: Arc<Mutex<()>>,
}

impl DiagnosticRetention {
    pub fn new(database: Database, storage_location: String) -> Self {
        Self {
            database,
            storage_location: storage_location.into(),
            secure_store: Arc::new(PlatformSecureKeyStore),
            operations: Arc::new(Mutex::new(())),
        }
    }

    #[cfg(test)]
    fn with_secure_store(
        database: Database,
        storage_location: String,
        secure_store: Arc<dyn SecureKeyStore>,
    ) -> Self {
        Self {
            database,
            storage_location: storage_location.into(),
            secure_store,
            operations: Arc::new(Mutex::new(())),
        }
    }

    pub fn status(&self) -> AppResult<DiagnosticRetentionStatus> {
        let _guard = self.operation_guard()?;
        self.status_at(Utc::now())
    }

    pub fn enable(&self) -> AppResult<DiagnosticRetentionStatus> {
        let _guard = self.operation_guard()?;
        self.enable_at(Utc::now())
    }

    pub fn clear(&self) -> AppResult<DiagnosticClearResult> {
        let _guard = self.operation_guard()?;
        self.clear_at(Utc::now())
    }

    pub fn expire_if_needed(&self) -> AppResult<DiagnosticRetentionStatus> {
        let _guard = self.operation_guard()?;
        self.status_at(Utc::now())
    }

    pub fn retain(&self, raw_envelope: &str) -> AppResult<bool> {
        let _guard = self.operation_guard()?;
        self.retain_at(raw_envelope, Utc::now())
    }

    fn operation_guard(&self) -> AppResult<std::sync::MutexGuard<'_, ()>> {
        self.operations
            .lock()
            .map_err(|_| AppError::InvalidRequest("diagnostic retention lock was poisoned".into()))
    }

    fn enable_at(&self, now: DateTime<Utc>) -> AppResult<DiagnosticRetentionStatus> {
        if self.status_at(now)?.enabled {
            return self.status_at(now);
        }
        let mut key = [0_u8; 32];
        SystemRandom::new()
            .fill(&mut key)
            .map_err(|_| AppError::InvalidRequest("secure random source is unavailable".into()))?;
        self.secure_store.store(&key).map_err(|_| {
            AppError::InvalidRequest(
                "platform secure storage is unavailable; diagnostic retention was not enabled"
                    .into(),
            )
        })?;
        let started_at = now.to_rfc3339();
        let expires_at = (now + Duration::days(RETENTION_DAYS)).to_rfc3339();
        if let Err(error) = self
            .database
            .start_diagnostic_retention_window(&started_at, &expires_at)
        {
            if self.secure_store.delete().is_err() {
                self.database.mark_diagnostic_retention_failed()?;
                return Err(AppError::InvalidRequest(
                    "diagnostic retention failed to start and its secure key could not be removed"
                        .into(),
                ));
            }
            return Err(error);
        }
        self.status_at(now)
    }

    fn status_at(&self, now: DateTime<Utc>) -> AppResult<DiagnosticRetentionStatus> {
        let Some((started_at, expires_at)) = self.database.diagnostic_retention_window()? else {
            if self.database.diagnostic_retention_failed()? {
                return Ok(self.disabled_status("unavailable", 0));
            }
            return Ok(self.disabled_status("disabled", 0));
        };
        let expiry = match DateTime::parse_from_rfc3339(&expires_at) {
            Ok(expiry) => expiry.with_timezone(&Utc),
            Err(_) => {
                let _ = self.clear_at(now)?;
                return Ok(self.disabled_status("unavailable", 0));
            }
        };
        if now >= expiry {
            let _ = self.clear_at(now)?;
            return Ok(self.disabled_status("expired", 0));
        }
        let retained_envelopes = self.database.diagnostic_envelope_count()?;
        if self.database.diagnostic_retention_failed()? {
            return Ok(DiagnosticRetentionStatus {
                state: "unavailable".into(),
                enabled: false,
                started_at: Some(started_at),
                expires_at: Some(expires_at),
                storage_location: self.storage_location.to_string(),
                retained_envelopes,
            });
        }
        match self.load_key() {
            Ok(_) => Ok(DiagnosticRetentionStatus {
                state: "active".into(),
                enabled: true,
                started_at: Some(started_at),
                expires_at: Some(expires_at),
                storage_location: self.storage_location.to_string(),
                retained_envelopes,
            }),
            Err(_) => Ok(DiagnosticRetentionStatus {
                state: "unavailable".into(),
                enabled: false,
                started_at: Some(started_at),
                expires_at: Some(expires_at),
                storage_location: self.storage_location.to_string(),
                retained_envelopes,
            }),
        }
    }

    fn clear_at(&self, _now: DateTime<Utc>) -> AppResult<DiagnosticClearResult> {
        let removed = self.database.clear_diagnostic_retention()?;
        if self.secure_store.delete().is_err() {
            self.database.mark_diagnostic_retention_failed()?;
            return Err(AppError::InvalidRequest(
                "diagnostic envelopes were removed but their secure key could not be removed"
                    .into(),
            ));
        }
        Ok(DiagnosticClearResult {
            removed,
            status: self.disabled_status("disabled", 0),
        })
    }

    fn retain_at(&self, raw_envelope: &str, now: DateTime<Utc>) -> AppResult<bool> {
        let result = self.try_retain_at(raw_envelope, now);
        if result.is_err() {
            self.database.mark_diagnostic_retention_failed()?;
        }
        result
    }

    fn try_retain_at(&self, raw_envelope: &str, now: DateTime<Utc>) -> AppResult<bool> {
        if self.database.diagnostic_retention_failed()? {
            return Ok(false);
        }
        let Some((_, expires_at)) = self.database.diagnostic_retention_window()? else {
            return Ok(false);
        };
        let expiry = DateTime::parse_from_rfc3339(&expires_at)
            .map_err(|_| AppError::InvalidRequest("diagnostic retention window is invalid".into()))?
            .with_timezone(&Utc);
        if now >= expiry {
            let _ = self.clear_at(now)?;
            return Ok(false);
        }
        self.database
            .purge_expired_diagnostic_envelopes(&now.to_rfc3339())?;
        let key = self.load_key()?;
        let mut nonce = [0_u8; 12];
        SystemRandom::new()
            .fill(&mut nonce)
            .map_err(|_| AppError::InvalidRequest("secure random source is unavailable".into()))?;
        let ciphertext = encrypt_envelope(&key, nonce, raw_envelope.as_bytes())?;
        self.database.retain_diagnostic_envelope(
            &now.to_rfc3339(),
            &expires_at,
            &nonce,
            &ciphertext,
        )?;
        Ok(true)
    }

    fn load_key(&self) -> AppResult<[u8; 32]> {
        let key = self.secure_store.load()?.ok_or_else(|| {
            AppError::InvalidRequest("diagnostic retention key is unavailable".into())
        })?;
        key.try_into().map_err(|_| {
            AppError::InvalidRequest("diagnostic retention key has an invalid length".into())
        })
    }

    fn disabled_status(&self, state: &str, retained_envelopes: u64) -> DiagnosticRetentionStatus {
        DiagnosticRetentionStatus {
            state: state.into(),
            enabled: false,
            started_at: None,
            expires_at: None,
            storage_location: self.storage_location.to_string(),
            retained_envelopes,
        }
    }
}

fn encrypt_envelope(key: &[u8; 32], nonce: [u8; 12], plaintext: &[u8]) -> AppResult<Vec<u8>> {
    let unbound = UnboundKey::new(&AES_256_GCM, key)
        .map_err(|_| AppError::InvalidRequest("diagnostic encryption key is invalid".into()))?;
    let key = LessSafeKey::new(unbound);
    let mut ciphertext = plaintext.to_vec();
    key.seal_in_place_append_tag(
        Nonce::assume_unique_for_key(nonce),
        Aad::from(ENVELOPE_AAD),
        &mut ciphertext,
    )
    .map_err(|_| AppError::InvalidRequest("diagnostic envelope encryption failed".into()))?;
    Ok(ciphertext)
}

struct PlatformSecureKeyStore;

#[cfg(target_os = "macos")]
impl SecureKeyStore for PlatformSecureKeyStore {
    fn load(&self) -> AppResult<Option<Vec<u8>>> {
        match security_framework::passwords::get_generic_password(
            KEYCHAIN_SERVICE,
            KEYCHAIN_ACCOUNT,
        ) {
            Ok(key) => Ok(Some(key)),
            Err(error) if error.code() == security_framework_sys::base::errSecItemNotFound => {
                Ok(None)
            }
            Err(_) => Err(AppError::InvalidRequest(
                "platform secure storage is unavailable".into(),
            )),
        }
    }

    fn store(&self, key: &[u8]) -> AppResult<()> {
        security_framework::passwords::set_generic_password(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT, key)
            .map_err(|_| AppError::InvalidRequest("platform secure storage is unavailable".into()))
    }

    fn delete(&self) -> AppResult<()> {
        match security_framework::passwords::delete_generic_password(
            KEYCHAIN_SERVICE,
            KEYCHAIN_ACCOUNT,
        ) {
            Ok(()) => Ok(()),
            Err(error) if error.code() == security_framework_sys::base::errSecItemNotFound => {
                Ok(())
            }
            Err(_) => Err(AppError::InvalidRequest(
                "platform secure storage is unavailable".into(),
            )),
        }
    }
}

#[cfg(not(target_os = "macos"))]
impl SecureKeyStore for PlatformSecureKeyStore {
    fn load(&self) -> AppResult<Option<Vec<u8>>> {
        Err(AppError::InvalidRequest(
            "platform secure storage is unavailable".into(),
        ))
    }

    fn store(&self, _key: &[u8]) -> AppResult<()> {
        Err(AppError::InvalidRequest(
            "platform secure storage is unavailable".into(),
        ))
    }

    fn delete(&self) -> AppResult<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[derive(Default)]
    struct MemorySecureKeyStore {
        key: Mutex<Option<Vec<u8>>>,
        unavailable: AtomicBool,
        delete_unavailable: AtomicBool,
    }

    impl SecureKeyStore for MemorySecureKeyStore {
        fn load(&self) -> AppResult<Option<Vec<u8>>> {
            if self.unavailable.load(Ordering::SeqCst) {
                return Err(AppError::InvalidRequest("secure store unavailable".into()));
            }
            Ok(self.key.lock().expect("test key lock").clone())
        }

        fn store(&self, key: &[u8]) -> AppResult<()> {
            if self.unavailable.load(Ordering::SeqCst) {
                return Err(AppError::InvalidRequest("secure store unavailable".into()));
            }
            *self.key.lock().expect("test key lock") = Some(key.to_vec());
            Ok(())
        }

        fn delete(&self) -> AppResult<()> {
            if self.delete_unavailable.load(Ordering::SeqCst) {
                return Err(AppError::InvalidRequest("secure store unavailable".into()));
            }
            *self.key.lock().expect("test key lock") = None;
            Ok(())
        }
    }

    fn fixture(
        secure_store: Arc<MemorySecureKeyStore>,
    ) -> (tempfile::TempDir, Database, DiagnosticRetention) {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let database = Database::open(temporary.path().join("diagnostics.sqlite"))
            .expect("database should open");
        let retention = DiagnosticRetention::with_secure_store(
            database.clone(),
            temporary.path().display().to_string(),
            secure_store,
        );
        (temporary, database, retention)
    }

    #[test]
    fn diagnostic_mode_encrypts_raw_envelopes_and_survives_restart() {
        let secure_store = Arc::new(MemorySecureKeyStore::default());
        let (_temporary, database, retention) = fixture(secure_store.clone());
        let started = DateTime::parse_from_rfc3339("2026-08-10T00:00:00Z")
            .expect("test time")
            .with_timezone(&Utc);
        let enabled = retention.enable_at(started).expect("mode should enable");
        assert!(enabled.enabled);
        assert_eq!(enabled.state, "active");
        assert_eq!(
            enabled.expires_at.as_deref(),
            Some("2026-08-17T00:00:00+00:00")
        );
        let raw = r#"{"prompt":"private prompt","command":"secret command"}"#;
        assert!(
            retention
                .retain_at(raw, started + Duration::hours(1))
                .expect("raw envelope should encrypt")
        );

        let (nonce, ciphertext) = database
            .diagnostic_envelope_rows()
            .expect("encrypted rows should load")
            .into_iter()
            .next()
            .expect("encrypted row should exist");
        assert_eq!(nonce.len(), 12);
        assert!(
            !ciphertext
                .windows(raw.len())
                .any(|window| window == raw.as_bytes())
        );
        let restarted = DiagnosticRetention::with_secure_store(
            database,
            "encrypted application data".into(),
            secure_store,
        );
        let status = restarted
            .status_at(started + Duration::days(1))
            .expect("restarted status should load");
        assert!(status.enabled);
        assert_eq!(status.retained_envelopes, 1);
    }

    #[test]
    fn diagnostic_mode_refuses_plaintext_fallback_without_secure_storage() {
        let secure_store = Arc::new(MemorySecureKeyStore {
            unavailable: AtomicBool::new(true),
            ..MemorySecureKeyStore::default()
        });
        let (_temporary, database, retention) = fixture(secure_store);
        let now = Utc::now();
        assert!(retention.enable_at(now).is_err());
        assert_eq!(
            database
                .diagnostic_retention_window()
                .expect("window should query"),
            None
        );
        assert_eq!(
            database
                .diagnostic_envelope_count()
                .expect("count should query"),
            0
        );
    }

    #[test]
    fn diagnostic_envelopes_expire_after_seven_days_and_can_be_cleared_early() {
        let secure_store = Arc::new(MemorySecureKeyStore::default());
        let (_temporary, database, retention) = fixture(secure_store.clone());
        let started = DateTime::parse_from_rfc3339("2026-08-10T00:00:00Z")
            .expect("test time")
            .with_timezone(&Utc);
        retention.enable_at(started).expect("mode should enable");
        retention
            .retain_at("private envelope", started + Duration::hours(1))
            .expect("envelope should retain");
        let cleared = retention
            .clear_at(started + Duration::days(2))
            .expect("early clear should succeed");
        assert_eq!(cleared.removed, 1);
        assert!(!cleared.status.enabled);

        retention.enable_at(started).expect("mode should re-enable");
        retention
            .retain_at("second private envelope", started + Duration::days(1))
            .expect("second envelope should retain");
        let expired = retention
            .status_at(started + Duration::days(7))
            .expect("expiry should run");
        assert_eq!(expired.state, "expired");
        assert_eq!(
            database
                .diagnostic_envelope_count()
                .expect("expired count should query"),
            0
        );
        assert!(secure_store.key.lock().expect("test key lock").is_none());
    }

    #[test]
    fn missing_restart_key_reports_unavailable_without_exposing_ciphertext() {
        let secure_store = Arc::new(MemorySecureKeyStore::default());
        let (_temporary, database, retention) = fixture(secure_store.clone());
        let started = Utc::now();
        retention.enable_at(started).expect("mode should enable");
        retention
            .retain_at("private envelope", started)
            .expect("envelope should retain");
        *secure_store.key.lock().expect("test key lock") = None;

        let status = retention.status_at(started).expect("status should load");
        assert_eq!(status.state, "unavailable");
        assert!(!status.enabled);
        assert_eq!(status.retained_envelopes, 1);
        let ciphertext = database
            .diagnostic_envelope_rows()
            .expect("ciphertext should remain queryable")
            .into_iter()
            .next()
            .expect("ciphertext should exist")
            .1;
        assert!(
            !ciphertext
                .windows("private envelope".len())
                .any(|window| window == b"private envelope")
        );
    }

    #[test]
    fn secure_key_deletion_failure_is_reported_until_cleanup_can_retry() {
        let secure_store = Arc::new(MemorySecureKeyStore::default());
        let (_temporary, database, retention) = fixture(secure_store.clone());
        let started = Utc::now();
        retention.enable_at(started).expect("mode should enable");
        retention
            .retain_at("private envelope", started)
            .expect("envelope should retain");
        secure_store
            .delete_unavailable
            .store(true, Ordering::SeqCst);

        assert!(retention.clear_at(started).is_err());
        assert_eq!(
            database
                .diagnostic_envelope_count()
                .expect("cleared count should query"),
            0
        );
        assert_eq!(
            retention
                .status_at(started)
                .expect("failed cleanup status should load")
                .state,
            "unavailable"
        );

        secure_store
            .delete_unavailable
            .store(false, Ordering::SeqCst);
        let retried = retention
            .clear_at(started)
            .expect("secure cleanup should retry");
        assert_eq!(retried.removed, 0);
        assert!(secure_store.key.lock().expect("test key lock").is_none());
        assert_eq!(
            retention
                .status_at(started)
                .expect("disabled status should load")
                .state,
            "disabled"
        );
    }

    #[test]
    fn expiry_reports_secure_key_cleanup_failure_after_removing_envelopes() {
        let secure_store = Arc::new(MemorySecureKeyStore::default());
        let (_temporary, database, retention) = fixture(secure_store.clone());
        let started = Utc::now();
        retention.enable_at(started).expect("mode should enable");
        retention
            .retain_at("private envelope", started)
            .expect("envelope should retain");
        secure_store
            .delete_unavailable
            .store(true, Ordering::SeqCst);

        assert!(retention.status_at(started + Duration::days(7)).is_err());
        assert_eq!(
            database
                .diagnostic_envelope_count()
                .expect("expired count should query"),
            0
        );
        assert_eq!(
            retention
                .status_at(started + Duration::days(7))
                .expect("failed expiry cleanup should be visible")
                .state,
            "unavailable"
        );
    }

    #[test]
    fn failed_enable_reports_when_its_secure_key_cannot_be_rolled_back() {
        let secure_store = Arc::new(MemorySecureKeyStore::default());
        let (temporary, _database, retention) = fixture(secure_store.clone());
        rusqlite::Connection::open(temporary.path().join("diagnostics.sqlite"))
            .expect("database should connect")
            .execute_batch(
                "CREATE TRIGGER reject_diagnostic_start
                 BEFORE INSERT ON app_settings
                 WHEN NEW.key='diagnosticRetentionStartedAt'
                 BEGIN SELECT RAISE(ABORT, 'diagnostic start rejected'); END;",
            )
            .expect("failure trigger should install");
        secure_store
            .delete_unavailable
            .store(true, Ordering::SeqCst);

        assert!(retention.enable_at(Utc::now()).is_err());
        assert_eq!(
            retention
                .status_at(Utc::now())
                .expect("failed rollback status should load")
                .state,
            "unavailable"
        );
        assert!(secure_store.key.lock().expect("test key lock").is_some());
    }

    #[test]
    fn retention_failure_becomes_visible_without_losing_the_canonical_event() {
        let secure_store = Arc::new(MemorySecureKeyStore::default());
        let (_temporary, database, retention) = fixture(secure_store.clone());
        let started = Utc::now();
        retention.enable_at(started).expect("mode should enable");
        database
            .record_live_event(
                &started.to_rfc3339(),
                &(started + Duration::days(90)).to_rfc3339(),
                "codex",
                "diagnostic-failure-session",
                "PermissionRequest",
                "project",
                "{}",
                "waiting",
            )
            .expect("canonical event should persist first");
        secure_store.unavailable.store(true, Ordering::SeqCst);

        assert!(retention.retain_at("private envelope", started).is_err());
        let status = retention
            .status_at(started)
            .expect("retention failure status should load");
        assert_eq!(status.state, "unavailable");
        assert!(!status.enabled);
        assert_eq!(status.retained_envelopes, 0);
        secure_store.unavailable.store(false, Ordering::SeqCst);
        assert!(
            !retention
                .retain_at("private envelope after recovery", started)
                .expect("failed mode must remain stopped")
        );
        assert_eq!(
            database
                .diagnostic_envelope_count()
                .expect("stopped retention count should query"),
            0
        );
        let reenabled = retention
            .enable_at(started + Duration::minutes(1))
            .expect("user can explicitly re-enable after recovery");
        assert!(reenabled.enabled);
        assert!(
            retention
                .retain_at(
                    "private envelope after explicit re-enable",
                    started + Duration::minutes(1),
                )
                .expect("re-enabled mode should retain")
        );
        assert_eq!(
            database
                .diagnostic_envelope_count()
                .expect("re-enabled retention count should query"),
            1
        );
        assert_eq!(
            database
                .live_activity()
                .expect("canonical activity should remain available")
                .timeline
                .len(),
            1
        );
    }
}
