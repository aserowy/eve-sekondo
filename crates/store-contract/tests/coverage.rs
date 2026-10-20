//! Every method of every store trait is exercised by a contract case. A method that is only ever
//! called to set something up still counts as exercised when its result is read back by a case,
//! so this checks the weaker thing that can be checked without running anything: that a case
//! calls it.

use std::collections::BTreeMap;

const TRAITS: &str = include_str!("../../sekondo/src/store/mod.rs");

/// The method names of every `pub trait` in the store definitions.
fn methods() -> BTreeMap<String, Vec<String>> {
    let mut found = BTreeMap::new();
    let mut current: Option<String> = None;
    for line in TRAITS.lines() {
        if let Some(rest) = line.strip_prefix("pub trait ") {
            let name = rest.split(':').next().unwrap().trim().to_owned();
            found.insert(name.clone(), Vec::new());
            current = Some(name);
        } else if line == "}" {
            current = None;
        } else if let (Some(trait_name), Some(rest)) =
            (&current, line.trim_start().strip_prefix("async fn "))
        {
            let name = rest.split(['(', '<']).next().unwrap().trim();
            found.get_mut(trait_name).unwrap().push(name.to_owned());
        }
    }
    found
}

/// The source of every case file, without the helpers they share.
fn cases() -> String {
    [
        include_str!("../src/cases/accounts.rs"),
        include_str!("../src/cases/fees.rs"),
        include_str!("../src/cases/market.rs"),
        include_str!("../src/cases/opportunities.rs"),
        include_str!("../src/cases/orders.rs"),
        include_str!("../src/cases/sessions.rs"),
        include_str!("../src/cases/station.rs"),
        include_str!("../src/cases/trades.rs"),
        include_str!("../src/cases/wealth.rs"),
    ]
    .join("\n")
}

#[test]
fn every_trait_is_found() {
    let found = methods();
    let names: Vec<&str> = found.keys().map(String::as_str).collect();
    assert_eq!(
        names,
        [
            "AccountStore",
            "FeeStore",
            "MarketStore",
            "OpportunityStore",
            "OrderStore",
            "SessionData",
            "StationStore",
            "TradeStore",
            "WealthStore",
        ]
    );
    assert!(found.values().all(|methods| !methods.is_empty()));
}

#[test]
fn every_store_method_has_a_contract_case() {
    let cases = cases();
    let missing: Vec<String> = methods()
        .into_iter()
        .flat_map(|(trait_name, methods)| {
            methods
                .into_iter()
                .map(move |method| (trait_name.clone(), method))
        })
        .filter(|(_, method)| !cases.contains(&format!(".{method}(")))
        .map(|(trait_name, method)| format!("{trait_name}::{method}"))
        .collect();
    assert!(
        missing.is_empty(),
        "these store methods have no contract case; add one to crates/store-contract/src/cases: {missing:#?}"
    );
}
