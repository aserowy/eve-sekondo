//! `cargo run -p esi-verify`: holds the contracts against real EVE and records the verified stub.

use std::process::{Command, ExitCode};

use esi_verify::{Hooks, Settings, SystemBrowser, run};

fn stub_conforms() -> Result<(), String> {
    let status = Command::new("cargo")
        .args(["test", "-p", "eve-stub", "--test", "contract"])
        .current_dir(esi_contract::gate::workspace_root())
        .status()
        .map_err(|e| format!("cannot run cargo: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err("`cargo test -p eve-stub --test contract` failed".to_owned())
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    let root = esi_contract::gate::workspace_root();
    let _ = dotenvy::from_path(root.join(".env"));
    let settings = match Settings::from_lookup(|name| std::env::var(name).ok(), root)
        .and_then(|s| s.require_real_eve().map(|()| s))
    {
        Ok(settings) => settings,
        Err(message) => {
            eprintln!("esi-verify: {message}");
            return ExitCode::FAILURE;
        }
    };
    let hooks = Hooks {
        stub_conforms: &stub_conforms,
    };
    match run(&settings, &SystemBrowser, &hooks, &mut std::io::stdout()).await {
        Ok(_) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("esi-verify: {e}");
            ExitCode::FAILURE
        }
    }
}
