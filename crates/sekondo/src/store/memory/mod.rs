//! An in-memory implementation of every store, for tests. It computes what the SQL of the
//! Postgres store computes, naively and in Rust. The contract cases hold it to the Postgres store.

use std::sync::{Arc, Mutex, MutexGuard};

use time::OffsetDateTime;

use crate::{clock::Clock, store::Stores};

use tables::Tables;

mod account;
mod fee;
mod market;
pub(crate) mod numeric;
mod opportunity;
mod order;
mod session;
mod station;
mod tables;
mod trade;
mod wealth;

/// Everything the stores share: the tables and the clock.
pub(super) struct Shared {
    tables: Mutex<Tables>,
    clock: Arc<dyn Clock>,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, Tables> {
        self.tables.lock().expect("the tables are never poisoned")
    }

    /// The database's `now()`: microseconds, like `timestamptz`.
    pub(super) fn now(&self) -> OffsetDateTime {
        micros(self.clock.now())
    }
}

/// The wait after a failure: `base × 2^min(failures, 20)` seconds, but at most `cap`. The SQL
/// computes it in `float8` seconds and `interval` keeps microseconds.
pub(super) fn backoff(base: time::Duration, failures: i32, cap: time::Duration) -> time::Duration {
    let seconds = (base.as_seconds_f64() * 2f64.powi(failures.min(20))).min(cap.as_seconds_f64());
    time::Duration::microseconds((seconds * 1_000_000.0).round() as i64)
}

/// The bucket ends of an over-time series over `range` ending at `now`: every bucket end after the
/// range's start, the last one clamped to `now`. Buckets are aligned to the origin
/// `time_bucket` uses (a Monday midnight, UTC).
pub(super) fn bucket_ends(range: crate::range::Range, now: OffsetDateTime) -> Vec<OffsetDateTime> {
    const ORIGIN: i64 = 946_857_600; // 2000-01-03 00:00:00 UTC
    let width = range.bucket().whole_microseconds() as i64;
    let start = now - range.span();
    let micros_of = |t: OffsetDateTime| (t.unix_timestamp_nanos() / 1000) as i64;
    let from_micros = |m: i64| {
        OffsetDateTime::from_unix_timestamp_nanos(i128::from(m) * 1000).expect("a valid time")
    };
    let origin = ORIGIN * 1_000_000;
    let bucket = |t: OffsetDateTime| (micros_of(t) - origin).div_euclid(width) * width + origin;
    let now_micros = micros_of(now);
    let mut ends = Vec::new();
    let mut end = bucket(start) + width;
    while end <= now_micros {
        if end < now_micros {
            ends.push(from_micros(end));
        }
        end += width;
    }
    ends.push(now);
    ends
}

/// `timestamptz` keeps microseconds.
pub(super) fn micros(at: OffsetDateTime) -> OffsetDateTime {
    at.replace_nanosecond(at.nanosecond() / 1000 * 1000)
        .expect("a valid nanosecond")
}

/// The in-memory stores over one set of tables.
#[derive(Clone)]
pub struct MemoryStores {
    shared: Arc<Shared>,
}

impl MemoryStores {
    pub fn new(clock: Arc<dyn Clock>) -> Self {
        Self {
            shared: Arc::new(Shared {
                tables: Mutex::new(Tables::default()),
                clock,
            }),
        }
    }

    pub fn stores(&self) -> Stores {
        let shared = || self.shared.clone();
        Stores {
            accounts: Arc::new(account::MemAccountStore { shared: shared() }),
            sessions: Arc::new(session::MemSessionStore { shared: shared() }),
            orders: Arc::new(order::MemOrderStore { shared: shared() }),
            market: Arc::new(market::MemMarketStore { shared: shared() }),
            station: Arc::new(station::MemStationStore { shared: shared() }),
            opportunities: Arc::new(opportunity::MemOpportunityStore { shared: shared() }),
            wealth: Arc::new(wealth::MemWealthStore { shared: shared() }),
            fees: Arc::new(fee::MemFeeStore { shared: shared() }),
            trades: Arc::new(trade::MemTradeStore { shared: shared() }),
        }
    }
}
