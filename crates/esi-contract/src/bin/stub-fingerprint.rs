//! `stub-fingerprint` prints the stub's fingerprint; `stub-fingerprint --check` exits non-zero,
//! with the gate's message, unless the recorded verification matches the stub.

use std::process::ExitCode;

use esi_contract::{fingerprint, gate};

fn main() -> ExitCode {
    let root = gate::workspace_root();
    if std::env::args().nth(1).as_deref() == Some("--check") {
        return match gate::check_verified(&root) {
            Ok(()) => ExitCode::SUCCESS,
            Err(message) => {
                eprintln!("{message}");
                ExitCode::FAILURE
            }
        };
    }
    match fingerprint(&root) {
        Ok(print) => {
            println!("{print}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("cannot fingerprint the stub: {e}");
            ExitCode::FAILURE
        }
    }
}
