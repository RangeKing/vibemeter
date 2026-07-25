use crate::errors::{AppError, AppResult};
use rusqlite::{Connection, MAIN_DB, OpenFlags};
use std::path::{Path, PathBuf};

const AFTERVIBE_BUNDLE_ID: &str = "com.aftervibe.desktop";
const AFTERVIBE_DATABASE_NAME: &str = "aftervibe.sqlite";
const TOKENGRAPH_BUNDLE_ID: &str = "com.tokengraph.desktop";
const TOKENGRAPH_DATABASE_NAME: &str = "TokenGraph.sqlite";
const DATABASE_NAME: &str = "vibemeter.sqlite";

pub fn prepare_database(data_dir: &Path) -> AppResult<PathBuf> {
    std::fs::create_dir_all(data_dir)?;
    let target = data_dir.join(DATABASE_NAME);
    if target.exists() {
        return Ok(target);
    }

    let Some(application_support) = dirs::data_dir() else {
        return Ok(target);
    };
    let candidates = migration_candidates(&application_support);
    if let Some(source) = candidates.iter().find(|candidate| candidate.is_file()) {
        migrate_database(source, &target)?;
    }
    Ok(target)
}

fn migration_candidates(application_support: &Path) -> [PathBuf; 2] {
    [
        application_support
            .join(AFTERVIBE_BUNDLE_ID)
            .join(AFTERVIBE_DATABASE_NAME),
        application_support
            .join(TOKENGRAPH_BUNDLE_ID)
            .join(TOKENGRAPH_DATABASE_NAME),
    ]
}

fn migrate_database(legacy: &Path, target: &Path) -> AppResult<()> {
    let staging = target.with_extension("sqlite.migrating");
    if staging.exists() {
        std::fs::remove_file(&staging)?;
    }

    let source = Connection::open_with_flags(
        legacy,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    source.busy_timeout(std::time::Duration::from_secs(10))?;
    if let Err(error) = source.backup(MAIN_DB, &staging, None) {
        let _ = std::fs::remove_file(&staging);
        return Err(AppError::Database(error));
    }

    let migrated = Connection::open_with_flags(
        &staging,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    let quick_check: String = migrated.query_row("PRAGMA quick_check", [], |row| row.get(0))?;
    if quick_check != "ok" {
        drop(migrated);
        let _ = std::fs::remove_file(&staging);
        return Err(AppError::InvalidRequest(
            "legacy database copy did not pass integrity verification".into(),
        ));
    }
    drop(migrated);
    std::fs::rename(staging, target)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copies_legacy_database_without_touching_the_source() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let legacy = temporary.path().join("TokenGraph.sqlite");
        let target = temporary.path().join("vibemeter.sqlite");
        let source = Connection::open(&legacy).expect("legacy database");
        source
            .execute_batch(
                "CREATE TABLE sessions(id TEXT PRIMARY KEY);\n\
                 INSERT INTO sessions(id) VALUES('one'),('two');\n\
                 PRAGMA user_version=2;",
            )
            .expect("legacy fixture");
        drop(source);

        migrate_database(&legacy, &target).expect("migration");

        let migrated = Connection::open(target).expect("migrated database");
        let count: i64 = migrated
            .query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))
            .expect("session count");
        assert_eq!(count, 2);
        assert!(legacy.exists());
    }

    #[test]
    fn prefers_aftervibe_before_the_older_tokengraph_database() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let candidates = migration_candidates(temporary.path());
        assert_eq!(
            candidates[0],
            temporary
                .path()
                .join("com.aftervibe.desktop")
                .join("aftervibe.sqlite")
        );
        assert_eq!(
            candidates[1],
            temporary
                .path()
                .join("com.tokengraph.desktop")
                .join("TokenGraph.sqlite")
        );
    }
}
