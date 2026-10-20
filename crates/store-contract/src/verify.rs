//! Running the contract cases and recording the outcome.

use std::{any::Any, io, panic::AssertUnwindSafe, path::Path};

use futures_util::FutureExt;
use sekondo::store::Stores;
use time::OffsetDateTime;

use crate::{
    cases::Case,
    fingerprint::fingerprint,
    gate::{Record, write_record},
};

/// A case that failed on one of the stores, with what it said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub store: String,
    pub case: String,
    pub message: String,
}

fn panic_message(payload: Box<dyn Any + Send>) -> String {
    match payload.downcast::<String>() {
        Ok(message) => *message,
        Err(payload) => payload
            .downcast_ref::<&str>()
            .map_or_else(|| "the case panicked".to_owned(), |m| (*m).to_owned()),
    }
}

/// Runs one case against the stores. An assertion that fails is a `Failure`, not a crash.
pub async fn run_case(store: &str, case: &Case, stores: &Stores) -> Result<(), Failure> {
    catch_case(store, case.name, (case.run)(stores)).await
}

/// Awaits a case's future and turns a panic in it into a `Failure`.
pub async fn catch_case(
    store: &str,
    case: &str,
    run: impl std::future::Future<Output = ()>,
) -> Result<(), Failure> {
    AssertUnwindSafe(run)
        .catch_unwind()
        .await
        .map_err(|payload| Failure {
            store: store.to_owned(),
            case: case.to_owned(),
            message: panic_message(payload),
        })
}

/// Writes the record of a verification when nothing failed, and leaves the recorded file alone
/// otherwise. Answers whether it wrote.
pub fn record_if_clean(
    workspace_root: &Path,
    failures: &[Failure],
    cases: &[Case],
    verified_at: OffsetDateTime,
) -> io::Result<bool> {
    if !failures.is_empty() {
        return Ok(false);
    }
    let record = Record {
        fingerprint: fingerprint(workspace_root)?,
        verified_at: verified_at
            .format(&time::format_description::well_known::Rfc3339)
            .map_err(io::Error::other)?,
        cases: cases.iter().map(|c| c.name.to_owned()).collect(),
    };
    write_record(workspace_root, &record)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;
    use crate::gate::{check_verified, read_record, record_path};

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

    fn passing(_: &Stores) -> crate::cases::CaseFuture<'_> {
        Box::pin(async {})
    }

    fn failure(case: &str) -> Failure {
        Failure {
            store: "postgres".to_owned(),
            case: case.to_owned(),
            message: "left 2 right 3".to_owned(),
        }
    }

    #[test]
    fn a_clean_run_records_the_current_fingerprint_and_every_case() {
        let dir = workspace();
        let cases = [Case {
            name: "dummy/passes",
            run: passing,
        }];

        let wrote = record_if_clean(dir.path(), &[], &cases, OffsetDateTime::now_utc()).unwrap();

        assert!(wrote);
        assert_eq!(read_record(dir.path()).unwrap().cases, ["dummy/passes"]);
        assert_eq!(check_verified(dir.path()), Ok(()));
    }

    #[test]
    fn a_run_with_zero_cases_still_records() {
        let dir = workspace();

        assert!(record_if_clean(dir.path(), &[], &[], OffsetDateTime::now_utc()).unwrap());
        assert_eq!(check_verified(dir.path()), Ok(()));
    }

    #[test]
    fn a_failing_case_leaves_the_recorded_file_unchanged() {
        let dir = workspace();
        let before = fingerprint(dir.path()).unwrap();
        record_if_clean(dir.path(), &[], &[], OffsetDateTime::now_utc()).unwrap();
        let recorded = std::fs::read_to_string(record_path(dir.path())).unwrap();
        std::fs::write(
            dir.path().join("crates/sekondo/migrations/001.sql"),
            "edited",
        )
        .unwrap();
        assert_ne!(before, fingerprint(dir.path()).unwrap());

        let wrote = record_if_clean(
            dir.path(),
            &[failure("dummy/fails")],
            &[],
            OffsetDateTime::now_utc(),
        )
        .unwrap();

        assert!(!wrote);
        assert_eq!(
            std::fs::read_to_string(record_path(dir.path())).unwrap(),
            recorded
        );
    }

    #[tokio::test]
    async fn a_failing_assertion_becomes_a_failure_naming_the_case_and_the_difference() {
        let outcome = catch_case("memory", "dummy/fails", async {
            assert_eq!(1 + 1, 3, "the sum");
        })
        .await;

        let failure = outcome.unwrap_err();
        assert_eq!(failure.case, "dummy/fails");
        assert_eq!(failure.store, "memory");
        assert!(failure.message.contains("the sum"), "{}", failure.message);
        assert!(failure.message.contains("left: 2"), "{}", failure.message);
    }

    #[tokio::test]
    async fn a_passing_case_is_not_a_failure() {
        assert_eq!(catch_case("memory", "dummy/passes", async {}).await, Ok(()));
    }
}
