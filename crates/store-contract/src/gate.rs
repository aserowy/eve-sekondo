//! The gate: tests on the in-memory store only run while the store is the one that was verified
//! against PostgreSQL.

use std::{
    io,
    path::{Path, PathBuf},
    sync::OnceLock,
};

use serde::{Deserialize, Serialize};

use crate::fingerprint::fingerprint;

pub const VERIFY_COMMAND: &str = "cargo run -p store-contract --bin store-verify";

/// What a successful verification leaves behind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    pub fingerprint: String,
    pub verified_at: String,
    /// The name of every contract case that passed against PostgreSQL.
    pub cases: Vec<String>,
}

/// The workspace root, as seen from this crate's source.
pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the workspace root exists")
}

pub fn record_path(workspace_root: &Path) -> PathBuf {
    workspace_root.join("crates/store-contract/verified-store.json")
}

pub fn read_record(workspace_root: &Path) -> io::Result<Record> {
    let text = std::fs::read_to_string(record_path(workspace_root))?;
    serde_json::from_str(&text).map_err(io::Error::other)
}

pub fn write_record(workspace_root: &Path, record: &Record) -> io::Result<()> {
    let mut text = serde_json::to_string_pretty(record).map_err(io::Error::other)?;
    text.push('\n');
    std::fs::write(record_path(workspace_root), text)
}

/// `Err(message)` unless the record matches the store sources in `workspace_root`.
pub fn check_verified(workspace_root: &Path) -> Result<(), String> {
    let record = read_record(workspace_root).map_err(|e| {
        format!(
            "the store has no recorded verification against PostgreSQL ({e}): run `{VERIFY_COMMAND}`"
        )
    })?;
    let current =
        fingerprint(workspace_root).map_err(|e| format!("cannot fingerprint the store: {e}"))?;
    if record.fingerprint == current {
        Ok(())
    } else {
        Err(format!(
            "the store changed since its last verification against PostgreSQL: run \
             `{VERIFY_COMMAND}` with the database up and commit \
             crates/store-contract/verified-store.json"
        ))
    }
}

/// Panics, before anything starts, unless the store is the verified one. There is no setting or
/// flag that gets around it.
pub fn require_verified_store() {
    static RESULT: OnceLock<Result<(), String>> = OnceLock::new();
    if let Err(message) = RESULT.get_or_init(|| check_verified(&workspace_root())) {
        panic!("{message}");
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    fn workspace() -> TempDir {
        let dir = TempDir::new().unwrap();
        for (path, contents) in [
            ("crates/sekondo/src/store/mod.rs", "traits"),
            ("crates/sekondo/migrations/001.sql", "create table a"),
            ("crates/store-contract/src/cases/a.rs", "cases"),
        ] {
            let path = dir.path().join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, contents).unwrap();
        }
        dir
    }

    fn record(fingerprint: String) -> Record {
        Record {
            fingerprint,
            verified_at: "2026-10-20T12:00:00Z".to_owned(),
            cases: vec!["orders/replace-keeps-snapshots".to_owned()],
        }
    }

    #[test]
    fn a_missing_record_fails_and_names_the_command() {
        let dir = workspace();
        let message = check_verified(dir.path()).unwrap_err();
        assert!(message.contains(VERIFY_COMMAND), "{message}");
    }

    #[test]
    fn a_stale_record_fails_and_names_the_command() {
        let dir = workspace();
        write_record(dir.path(), &record("0".repeat(64))).unwrap();
        let message = check_verified(dir.path()).unwrap_err();
        assert!(
            message.contains("store changed since its last verification against PostgreSQL"),
            "{message}"
        );
        assert!(message.contains(VERIFY_COMMAND), "{message}");
    }

    #[test]
    fn a_matching_record_passes_until_a_covered_file_changes() {
        let dir = workspace();
        write_record(dir.path(), &record(fingerprint(dir.path()).unwrap())).unwrap();
        assert_eq!(check_verified(dir.path()), Ok(()));

        std::fs::write(
            dir.path().join("crates/sekondo/migrations/001.sql"),
            "create table a -- edited",
        )
        .unwrap();
        assert!(check_verified(dir.path()).is_err());
    }

    #[test]
    fn the_record_round_trips() {
        let dir = workspace();
        std::fs::create_dir_all(dir.path().join("crates/store-contract")).unwrap();
        let written = record("abc".to_owned());
        write_record(dir.path(), &written).unwrap();
        assert_eq!(read_record(dir.path()).unwrap(), written);
    }
}
