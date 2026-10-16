//! Contracts for the calls the app makes to ESI and EVE SSO.
//!
//! The stub's answers and the real services' answers are checked against the same contracts, so
//! the stub cannot drift away from what EVE really sends.

mod check;
mod contracts;
mod fingerprint;
pub mod gate;
mod jwt;
mod model;

pub use check::{Violation, ViolationKind, check};
pub use contracts::{canonical_json, contract, contracts, coverage};
pub use fingerprint::{STUB_SOURCE, fingerprint};
pub use jwt::jwt_payload;
pub use model::*;
