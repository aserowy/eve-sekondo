//! The store fingerprint: a SHA-256 over what decides whether the in-memory store can still be
//! trusted: the store traits and both implementations, the contract cases and every migration.

use std::{
    io,
    path::{Path, PathBuf},
};

use sha2::{Digest, Sha256};

/// What the fingerprint covers, relative to the workspace root: the store traits with both
/// implementations, every migration and the contract cases.
pub const COVERED: [&str; 3] = [
    "crates/sekondo/src/store",
    "crates/sekondo/migrations",
    "crates/store-contract/src/cases",
];

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

/// Hex SHA-256 over every file under the covered directories, relative path and contents, in path
/// order, each in a length-prefixed section. Computed from the working tree, so an uncommitted
/// edit counts.
pub fn fingerprint(workspace_root: &Path) -> io::Result<String> {
    let mut files: Vec<(String, PathBuf)> = Vec::new();
    for covered in COVERED {
        let mut found = Vec::new();
        collect(&workspace_root.join(covered), &mut found)?;
        files.extend(found.into_iter().map(|path| {
            let relative = path
                .strip_prefix(workspace_root)
                .expect("under the workspace")
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            (relative, path)
        }));
    }
    files.sort();

    let mut hasher = Sha256::new();
    for (relative, path) in &files {
        let contents = std::fs::read(path)?;
        hasher.update((relative.len() as u64).to_be_bytes());
        hasher.update(relative.as_bytes());
        hasher.update((contents.len() as u64).to_be_bytes());
        hasher.update(&contents);
    }
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
            let path = dir.path().join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, contents).unwrap();
        }
        dir
    }

    fn base() -> Vec<(&'static str, &'static str)> {
        vec![
            ("crates/sekondo/src/store/mod.rs", "traits"),
            ("crates/sekondo/src/store/pg/orders.rs", "sql"),
            ("crates/sekondo/migrations/001_a.sql", "create table a"),
            ("crates/store-contract/src/cases/orders.rs", "cases"),
        ]
    }

    fn print(files: &[(&str, &str)]) -> String {
        fingerprint(tree(files).path()).unwrap()
    }

    fn with(extra: &[(&'static str, &'static str)]) -> Vec<(&'static str, &'static str)> {
        let mut files = base();
        files.extend_from_slice(extra);
        files
    }

    #[test]
    fn the_same_tree_gives_the_same_fingerprint_whatever_the_order_it_was_written_in() {
        let mut reversed = base();
        reversed.reverse();
        assert_eq!(print(&base()), print(&reversed));
        assert_eq!(print(&base()).len(), 64);
    }

    #[test]
    fn a_changed_store_file_changes_it() {
        let mut changed = base();
        changed[1].1 = "other sql";
        assert_ne!(print(&base()), print(&changed));
    }

    #[test]
    fn an_added_store_file_changes_it() {
        assert_ne!(
            print(&base()),
            print(&with(&[("crates/sekondo/src/store/memory/orders.rs", "")]))
        );
    }

    #[test]
    fn a_removed_store_file_changes_it() {
        assert_ne!(print(&base()), print(&base()[1..]));
    }

    #[test]
    fn moving_content_between_files_changes_it() {
        assert_ne!(
            print(&[
                ("crates/sekondo/src/store/a.rs", "xy"),
                ("crates/sekondo/src/store/b.rs", ""),
                ("crates/sekondo/migrations/m.sql", ""),
                ("crates/store-contract/src/cases/c.rs", ""),
            ]),
            print(&[
                ("crates/sekondo/src/store/a.rs", "x"),
                ("crates/sekondo/src/store/b.rs", "y"),
                ("crates/sekondo/migrations/m.sql", ""),
                ("crates/store-contract/src/cases/c.rs", ""),
            ])
        );
    }

    #[test]
    fn a_changed_migration_changes_it() {
        let mut changed = base();
        changed[2].1 = "create table a -- comment";
        assert_ne!(print(&base()), print(&changed));
    }

    #[test]
    fn a_changed_contract_case_changes_it() {
        let mut changed = base();
        changed[3].1 = "other cases";
        assert_ne!(print(&base()), print(&changed));
    }

    #[test]
    fn an_edit_outside_the_covered_paths_does_not_change_it() {
        assert_eq!(
            print(&base()),
            print(&with(&[
                ("crates/sekondo/src/orders.rs", "a handler"),
                ("crates/sekondo/templates/orders.html", "a page"),
                ("crates/sekondo/tests/poll.rs", "a test"),
                ("crates/eve-stub/src/lib.rs", "the stub"),
            ]))
        );
    }

    #[test]
    fn a_workspace_without_the_store_sources_is_an_error() {
        let empty = TempDir::new().unwrap();
        assert!(fingerprint(empty.path()).is_err());
    }
}
