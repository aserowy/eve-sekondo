//! The gate: stub-backed tests only run while the stub is the one that was verified against real
//! EVE.

use std::{
    io,
    path::{Path, PathBuf},
    sync::OnceLock,
};

use serde::{Deserialize, Serialize};

use crate::fingerprint::fingerprint;

pub const VERIFY_COMMAND: &str = "cargo run -p esi-verify";

/// What a successful verification leaves behind. No tokens, no character data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    pub fingerprint: String,
    pub verified_at: String,
    /// `call/variant` of every variant that was held against real EVE.
    pub variants: Vec<String>,
}

/// The workspace root, as seen from this crate's source.
pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the workspace root exists")
}

pub fn record_path(workspace_root: &Path) -> PathBuf {
    workspace_root.join("crates/esi-contract/verified-stub.json")
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

/// `Err(message)` unless the record matches the stub in `workspace_root`.
pub fn check_verified(workspace_root: &Path) -> Result<(), String> {
    let record = read_record(workspace_root).map_err(|e| {
        format!(
            "the stub has no recorded verification against real EVE ({e}): run `{VERIFY_COMMAND}`"
        )
    })?;
    let current =
        fingerprint(workspace_root).map_err(|e| format!("cannot fingerprint the stub: {e}"))?;
    if record.fingerprint == current {
        Ok(())
    } else {
        Err(format!(
            "the stub changed since it was last verified against real EVE: run `{VERIFY_COMMAND}` \
             and commit crates/esi-contract/verified-stub.json"
        ))
    }
}

/// Panics, before anything starts, unless the stub is the verified one. There is no way around
/// it by setting or flag.
pub fn require_verified_stub() {
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
        let src = dir.path().join("crates/eve-stub/src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("lib.rs"), "stub").unwrap();
        std::fs::create_dir_all(dir.path().join("crates/esi-contract")).unwrap();
        dir
    }

    fn record(fingerprint: String) -> Record {
        Record {
            fingerprint,
            verified_at: "2026-10-16T12:00:00Z".to_owned(),
            variants: vec!["orders/sell-orders".to_owned()],
        }
    }

    #[test]
    fn a_missing_record_fails_and_names_the_command() {
        let dir = workspace();
        let message = check_verified(dir.path()).unwrap_err();
        assert!(message.contains("cargo run -p esi-verify"), "{message}");
    }

    #[test]
    fn a_stale_fingerprint_fails_and_names_the_command() {
        let dir = workspace();
        write_record(dir.path(), &record("0".repeat(64))).unwrap();
        let message = check_verified(dir.path()).unwrap_err();
        assert!(
            message.contains("stub changed since it was last verified"),
            "{message}"
        );
        assert!(message.contains("cargo run -p esi-verify"), "{message}");
    }

    #[test]
    fn a_matching_record_passes_until_the_stub_changes() {
        let dir = workspace();
        write_record(dir.path(), &record(fingerprint(dir.path()).unwrap())).unwrap();
        assert_eq!(check_verified(dir.path()), Ok(()));

        std::fs::write(dir.path().join("crates/eve-stub/src/lib.rs"), "edited").unwrap();
        assert!(check_verified(dir.path()).is_err());
    }

    #[test]
    fn the_record_round_trips() {
        let dir = workspace();
        let written = record("abc".to_owned());
        write_record(dir.path(), &written).unwrap();
        assert_eq!(read_record(dir.path()).unwrap(), written);
    }
}
