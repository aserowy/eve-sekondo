//! The contract of the app's stores: cases that every store must pass, a fingerprint of what was
//! verified, and the gate that keeps tests off an unverified in-memory store.

pub mod cases;
pub mod fingerprint;
pub mod gate;
pub mod verify;
