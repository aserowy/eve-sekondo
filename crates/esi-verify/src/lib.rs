//! Verifies the contracts against real EVE SSO and ESI, and records the verified stub.

mod browser;
mod callback;
mod scrub;
mod settings;
mod verify;

pub use browser::{Browser, SystemBrowser, display_available};
pub use scrub::scrub_decode_error;
pub use settings::{REAL_ESI, REAL_SSO, Settings};
pub use verify::{Hooks, VerifyError, run};
