//! The contract cases. Each one writes through the stores and asserts on what they read back, so
//! the same case holds the in-memory store and PostgreSQL to the same behaviour.

use std::{future::Future, pin::Pin};

use sekondo::store::Stores;

mod accounts;
mod fees;
mod market;
mod opportunities;
mod orders;
mod sessions;
mod station;
pub mod support;
mod trades;
mod wealth;

pub type CaseFuture<'a> = Pin<Box<dyn Future<Output = ()> + Send + 'a>>;

/// One case: a name (`domain/what-it-checks`) and what it runs against a fresh, empty store.
#[derive(Clone, Copy)]
pub struct Case {
    pub name: &'static str,
    pub run: for<'a> fn(&'a Stores) -> CaseFuture<'a>,
}

/// Every registered case.
pub fn all() -> Vec<Case> {
    let mut all = Vec::new();
    all.extend(accounts::cases());
    all.extend(orders::cases());
    all.extend(market::cases());
    all.extend(station::cases());
    all.extend(opportunities::cases());
    all.extend(wealth::cases());
    all.extend(fees::cases());
    all.extend(trades::cases());
    all.extend(sessions::cases());
    all
}
