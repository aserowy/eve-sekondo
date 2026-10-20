//! Every contract case against the in-memory store. This run is not gated, so the fake can be
//! developed before it is verified; it is part of plain `cargo test`.

use std::sync::Arc;

use libtest_mimic::{Arguments, Failed, Trial};
use sekondo::{clock::SystemClock, store::memory::MemoryStores};
use store_contract::{cases, verify::run_case};

fn main() {
    let args = Arguments::from_args();
    let trials = cases::all()
        .into_iter()
        .map(|case| {
            Trial::test(case.name, move || {
                let stores = MemoryStores::new(Arc::new(SystemClock)).stores();
                let outcome = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("a runtime")
                    .block_on(run_case("memory", &case, &stores));
                outcome.map_err(|failure| Failed::from(failure.message))
            })
        })
        .collect();
    libtest_mimic::run(&args, trials).exit();
}
