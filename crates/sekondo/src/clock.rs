use std::fmt::Debug;

use time::OffsetDateTime;

/// Source of "now" for the session lifetime checks, so tests can move time.
pub trait Clock: Debug + Send + Sync + 'static {
    fn now(&self) -> OffsetDateTime;
}

#[derive(Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> OffsetDateTime {
        OffsetDateTime::now_utc()
    }
}
