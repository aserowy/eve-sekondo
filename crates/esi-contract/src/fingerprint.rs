//! The stub fingerprint: a SHA-256 over the stub's source and the contracts.

use std::{
    io,
    path::{Path, PathBuf},
};

use sha2::{Digest, Sha256};

use crate::contracts::canonical_json;

/// The stub's source, relative to the workspace root.
pub const STUB_SOURCE: &str = "crates/eve-stub/src";

fn collect(dir: &Path, found: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect(&path, found)?;
        } else {
            found.push(path);
        }
    }
    Ok(())
}

/// Hex SHA-256 over every file under the stub's `src` (relative path and contents, in path order)
/// and the canonical JSON of all contracts. Computed from the working tree, so an uncommitted
/// edit counts.
pub fn fingerprint(workspace_root: &Path) -> io::Result<String> {
    fingerprint_with(workspace_root, &canonical_json())
}

fn fingerprint_with(workspace_root: &Path, contracts_json: &str) -> io::Result<String> {
    let source = workspace_root.join(STUB_SOURCE);
    let mut files = Vec::new();
    collect(&source, &mut files)?;
    let mut files: Vec<(String, PathBuf)> = files
        .into_iter()
        .map(|path| {
            let relative = path
                .strip_prefix(&source)
                .expect("under the source dir")
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            (relative, path)
        })
        .collect();
    files.sort();

    let mut hasher = Sha256::new();
    let mut section = |label: &str, bytes: &[u8]| {
        hasher.update((label.len() as u64).to_be_bytes());
        hasher.update(label.as_bytes());
        hasher.update((bytes.len() as u64).to_be_bytes());
        hasher.update(bytes);
    };
    for (relative, path) in &files {
        section(&format!("stub:{relative}"), &std::fs::read(path)?);
    }
    section("contracts", contracts_json.as_bytes());
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    fn tree(files: &[(&str, &str)]) -> TempDir {
        let dir = TempDir::new().unwrap();
        for (name, contents) in files {
            let path = dir.path().join(STUB_SOURCE).join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, contents).unwrap();
        }
        dir
    }

    fn print(dir: &TempDir) -> String {
        fingerprint(dir.path()).unwrap()
    }

    #[test]
    fn the_same_tree_gives_the_same_fingerprint_whatever_the_order_on_disk() {
        let a = tree(&[("lib.rs", "one"), ("sub/x.rs", "two")]);
        let b = tree(&[("sub/x.rs", "two"), ("lib.rs", "one")]);
        assert_eq!(print(&a), print(&b));
        assert_eq!(print(&a).len(), 64);
    }

    #[test]
    fn a_changed_stub_file_changes_it() {
        assert_ne!(
            print(&tree(&[("lib.rs", "one")])),
            print(&tree(&[("lib.rs", "one!")]))
        );
    }

    #[test]
    fn an_added_stub_file_changes_it() {
        assert_ne!(
            print(&tree(&[("lib.rs", "one")])),
            print(&tree(&[("lib.rs", "one"), ("extra.rs", "")]))
        );
    }

    #[test]
    fn moving_content_between_files_changes_it() {
        assert_ne!(
            print(&tree(&[("a.rs", "xy"), ("b.rs", "")])),
            print(&tree(&[("a.rs", "x"), ("b.rs", "y")]))
        );
    }

    #[test]
    fn a_changed_contract_changes_it() {
        let dir = tree(&[("lib.rs", "one")]);
        assert_eq!(
            fingerprint(dir.path()).unwrap(),
            fingerprint_with(dir.path(), &canonical_json()).unwrap()
        );
        assert_ne!(
            fingerprint_with(dir.path(), &canonical_json()).unwrap(),
            fingerprint_with(dir.path(), &canonical_json().replace("sell-orders", "sell")).unwrap()
        );
    }

    #[test]
    fn a_missing_stub_source_is_an_error() {
        let empty = TempDir::new().unwrap();
        assert!(fingerprint(empty.path()).is_err());
    }
}
