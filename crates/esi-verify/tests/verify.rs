//! The verification against the stub, with the test playing the person at the browser.
//!
//! These tests are not gated on a verified stub: they exercise the verifier, which is what lifts
//! the gate.

use std::{
    cell::Cell,
    net::TcpListener,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

use esi_contract::gate::{self, Record};
use esi_verify::{Browser, Hooks, Settings, display_available, run};
use eve_stub::{Stub, StubConfig, pki::TestPki};
use secrecy::SecretString;
use serde_json::{Value, json};
use tempfile::TempDir;
use url::Url;

const CLIENT_ID: &str = "test-client";
const CLIENT_SECRET: &str = "test-secret";
const CONTROL_TOKEN: &str = "test-control-token";
const ALICE: i64 = 90000001;
const STRUCTURE: i64 = 1000000000001;
const FORBIDDEN_STRUCTURE: i64 = 1000000000002;

/// How the person behaves at the login page.
#[derive(Clone, Copy)]
enum Person {
    LogsIn,
    /// The callback arrives with a state that was not sent.
    ForgedState,
}

struct Visitor {
    client: reqwest::Client,
    person: Person,
    opened: AtomicUsize,
}

impl Browser for Visitor {
    fn ensure_available(&self) -> Result<(), String> {
        Ok(())
    }

    fn open(&self, url: &Url) -> Result<(), String> {
        self.opened.fetch_add(1, Ordering::SeqCst);
        let mut select = url.clone();
        select.set_path("/v2/oauth/authorize/select");
        select
            .query_pairs_mut()
            .append_pair("character_id", &ALICE.to_string());
        let (client, person) = (self.client.clone(), self.person);
        tokio::spawn(async move {
            let res = client.get(select).send().await.unwrap();
            let mut callback = Url::parse(res.headers()["location"].to_str().unwrap()).unwrap();
            if let Person::ForgedState = person {
                let pairs: Vec<(String, String)> = callback
                    .query_pairs()
                    .map(|(k, v)| {
                        let v = if k == "state" {
                            "forged".into()
                        } else {
                            v.into_owned()
                        };
                        (k.into_owned(), v)
                    })
                    .collect();
                callback.query_pairs_mut().clear().extend_pairs(pairs);
            }
            let _ = client.get(callback).send().await;
        });
        Ok(())
    }
}

struct NoBrowser;

impl Browser for NoBrowser {
    fn ensure_available(&self) -> Result<(), String> {
        display_available("linux", |_| None)
    }

    fn open(&self, _: &Url) -> Result<(), String> {
        panic!("must not open anything");
    }
}

struct World {
    stub: Stub,
    settings: Settings,
    visitor: Visitor,
    _dir: TempDir,
    workspace: PathBuf,
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

async fn world(person: Person) -> World {
    let pki = TestPki::generate();
    let dir = TempDir::new().unwrap();
    let (ca, cert, key) = pki.write(dir.path()).unwrap();
    let stub = Stub::start(
        StubConfig::new(CLIENT_ID, CLIENT_SECRET, CONTROL_TOKEN),
        TcpListener::bind("127.0.0.1:0").unwrap(),
        pki.cert_pem.clone().into_bytes(),
        pki.key_pem.clone().into_bytes(),
    )
    .await
    .unwrap();

    let workspace = dir.path().join("workspace");
    std::fs::create_dir_all(workspace.join("crates/eve-stub/src")).unwrap();
    std::fs::write(workspace.join("crates/eve-stub/src/lib.rs"), "stub").unwrap();
    std::fs::create_dir_all(workspace.join("crates/esi-contract")).unwrap();

    let base = Url::parse(&stub.base_url).unwrap();
    let settings = Settings {
        sso_base_url: base.clone(),
        esi_base_url: base,
        client_id: CLIENT_ID.to_owned(),
        client_secret: SecretString::from(CLIENT_SECRET),
        public_base_url: Url::parse(&format!("https://localhost:{}", free_port())).unwrap(),
        tls_cert_path: cert,
        tls_key_path: key,
        extra_ca_path: Some(ca),
        contact: Some("tests@example.com".to_owned()),
        structure_id: STRUCTURE,
        forbidden_structure_id: FORBIDDEN_STRUCTURE,
        workspace_root: workspace.clone(),
        login_timeout: std::time::Duration::from_secs(20),
    };
    let world = World {
        visitor: Visitor {
            client: pki.client(),
            person,
            opened: AtomicUsize::new(0),
        },
        stub,
        settings,
        _dir: dir,
        workspace,
    };
    world
        .control(
            "structure-access",
            json!({"structure_id": FORBIDDEN_STRUCTURE, "allowed": false}),
        )
        .await;
    world
}

impl World {
    async fn control(&self, path: &str, body: Value) {
        let res = self
            .visitor
            .client
            .post(format!("{}/_stub/{path}", self.stub.base_url))
            .bearer_auth(CONTROL_TOKEN)
            .json(&body)
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), 204);
    }

    async fn verify(&self) -> (Result<Record, String>, String) {
        self.verify_with(&self.visitor, &|| Ok(())).await
    }

    async fn verify_with(
        &self,
        browser: &dyn Browser,
        conforms: &dyn Fn() -> Result<(), String>,
    ) -> (Result<Record, String>, String) {
        let mut out = Vec::new();
        let hooks = Hooks {
            stub_conforms: conforms,
        };
        let result = run(&self.settings, browser, &hooks, &mut out)
            .await
            .map_err(|e| e.to_string());
        (result, String::from_utf8(out).unwrap())
    }

    fn record(&self) -> Option<Record> {
        gate::read_record(&self.workspace).ok()
    }

    fn write_old_record(&self) -> Record {
        let old = Record {
            fingerprint: "old".to_owned(),
            verified_at: "2026-01-01T00:00:00Z".to_owned(),
            variants: vec![],
        };
        gate::write_record(&self.workspace, &old).unwrap();
        old
    }
}

fn order(id: i64, buy: bool) -> Value {
    let mut order = json!({
        "order_id": id, "type_id": 34, "region_id": 10000002, "location_id": 60003760,
        "is_buy_order": buy, "is_corporation": false, "price": 1500000.0,
        "volume_remain": 1, "volume_total": 2, "range": "station",
        "issued": "2026-10-11T12:00:00Z", "duration": 90,
    });
    if buy {
        order["min_volume"] = json!(1);
    }
    order
}

#[tokio::test]
async fn a_passing_verification_records_the_fingerprint_and_the_variants() {
    let w = world(Person::LogsIn).await;

    let (result, output) = w.verify().await;

    let record = result.unwrap_or_else(|e| panic!("{e}\n{output}"));
    assert_eq!(
        record.fingerprint,
        esi_contract::fingerprint(&w.workspace).unwrap()
    );
    assert_eq!(w.record(), Some(record.clone()));
    for variant in [
        "authorization/request",
        "authorization/callback",
        "token/authorization-code",
        "token/refresh",
        "token/access-token-claims",
        "jwks/rsa-keys",
        "orders/sell-orders",
        "orders/buy-orders",
        "names/found",
        "names/unknown-id",
        "structures/found",
        "structures/forbidden",
    ] {
        assert!(record.variants.iter().any(|v| v == variant), "{variant}");
    }
    assert!(
        !record.variants.iter().any(|v| v.contains("rate-limited")),
        "failure variants cannot be verified"
    );
    assert_eq!(w.visitor.opened.load(Ordering::SeqCst), 1);
    assert!(
        output.contains("/v2/oauth/authorize?"),
        "the login URL is printed"
    );

    // The ESI calls carry the app's identification.
    let requests = w.stub.esi_requests();
    assert!(requests.len() >= 5);
    for r in requests {
        assert!(
            r.user_agent
                .as_deref()
                .is_some_and(|ua| ua.starts_with("Sekondo/")),
            "{r:?}"
        );
        assert_eq!(
            r.compatibility_date.as_deref(),
            Some(sekondo::esi::COMPATIBILITY_DATE)
        );
    }
}

#[tokio::test]
async fn nothing_secret_is_printed() {
    let w = world(Person::LogsIn).await;
    let (_, output) = w.verify().await;
    for secret in [
        "eyJ",
        "Bearer",
        "1500000",
        "Alice",
        "Tritanium",
        CLIENT_SECRET,
    ] {
        assert!(!output.contains(secret), "{secret} in:\n{output}");
    }
}

#[tokio::test]
async fn a_wrong_state_exchanges_no_code_and_records_nothing() {
    let w = world(Person::ForgedState).await;

    let (result, _) = w.verify().await;

    let message = result.unwrap_err();
    assert!(message.contains("state"), "{message}");
    assert_eq!(w.stub.token_calls(), 0);
    assert_eq!(w.record(), None);
}

#[tokio::test]
async fn a_busy_port_is_named() {
    let w = world(Person::LogsIn).await;
    let addr = w.settings.callback_addr().unwrap();
    let _busy = TcpListener::bind(addr).unwrap();

    let (result, _) = w.verify().await;

    let message = result.unwrap_err();
    assert!(message.contains(&addr.to_string()), "{message}");
    assert_eq!(w.visitor.opened.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn without_a_display_it_fails_at_once_and_touches_nothing() {
    let w = world(Person::LogsIn).await;

    let (result, _) = w.verify_with(&NoBrowser, &|| Ok(())).await;

    let message = result.unwrap_err();
    assert!(message.contains("browser and a person"), "{message}");
    assert_eq!(w.stub.token_calls(), 0);
    assert_eq!(w.stub.esi_calls("orders"), 0);
    // The callback port was never taken.
    TcpListener::bind(w.settings.callback_addr().unwrap()).unwrap();
}

#[tokio::test]
async fn a_stub_that_does_not_conform_is_not_verified_at_all() {
    let w = world(Person::LogsIn).await;
    let asked = Cell::new(0);

    let (result, _) = w
        .verify_with(&w.visitor, &|| {
            asked.set(asked.get() + 1);
            Err("contract test failed".to_owned())
        })
        .await;

    assert!(result.unwrap_err().contains("contract test failed"));
    assert_eq!(asked.get(), 1);
    assert_eq!(w.visitor.opened.load(Ordering::SeqCst), 0);
    assert_eq!(w.stub.token_calls(), 0);
}

#[tokio::test]
async fn a_contract_violation_names_the_field_and_leaves_the_record() {
    let w = world(Person::LogsIn).await;
    let old = w.write_old_record();
    let mut sell = order(1, false);
    sell["min_volume"] = json!(1);
    w.control(
        "orders",
        json!({"character_id": ALICE, "orders": [sell, order(2, true)]}),
    )
    .await;

    let (result, output) = w.verify().await;

    assert!(result.is_err());
    assert!(output.contains("orders/sell-orders"), "{output}");
    assert!(output.contains("$[0].min_volume"), "{output}");
    assert_eq!(w.record(), Some(old));
}

#[tokio::test]
async fn an_answer_the_app_cannot_decode_fails_with_the_decoding_error_but_no_value() {
    let w = world(Person::LogsIn).await;
    let old = w.write_old_record();
    // An integer for the contract, but too big for the app's 32-bit duration.
    let mut sell = order(1, false);
    sell["duration"] = json!(3000000123_i64);
    w.control(
        "orders",
        json!({"character_id": ALICE, "orders": [sell, order(2, true)]}),
    )
    .await;

    let (result, output) = w.verify().await;

    assert!(result.is_err());
    assert!(output.contains("cannot decode"), "{output}");
    assert!(output.contains("invalid value: integer"), "{output}");
    assert!(!output.contains("3000000123"), "{output}");
    assert_eq!(w.record(), Some(old));
}

#[tokio::test]
async fn a_character_without_a_buy_order_is_told_so() {
    let w = world(Person::LogsIn).await;
    let old = w.write_old_record();
    w.control(
        "orders",
        json!({"character_id": ALICE, "orders": [order(1, false)]}),
    )
    .await;

    let (result, output) = w.verify().await;

    assert!(result.is_err());
    assert!(output.contains("needs a buy order"), "{output}");
    assert_eq!(w.record(), Some(old));
}

#[tokio::test]
async fn a_character_without_a_sell_order_is_told_so() {
    let w = world(Person::LogsIn).await;
    w.control(
        "orders",
        json!({"character_id": ALICE, "orders": [order(1, true)]}),
    )
    .await;

    let (_, output) = w.verify().await;

    assert!(output.contains("needs a sell order"), "{output}");
}

#[tokio::test]
async fn the_error_or_rate_limit_stops_the_verification() {
    let w = world(Person::LogsIn).await;
    let old = w.write_old_record();
    w.control(
        "esi-fail-next",
        json!({"route": "orders", "status": 429, "headers": {"retry-after": "60"}}),
    )
    .await;

    let (result, _) = w.verify().await;

    let message = result.unwrap_err();
    assert!(message.contains("429"), "{message}");
    assert_eq!(w.stub.esi_calls("names"), 0);
    assert_eq!(w.stub.esi_calls("structures"), 0);
    assert_eq!(w.record(), Some(old));
}

#[tokio::test]
async fn a_forbidden_structure_that_is_allowed_fails_the_forbidden_variant() {
    let w = world(Person::LogsIn).await;
    w.control(
        "structure-access",
        json!({"structure_id": FORBIDDEN_STRUCTURE, "allowed": true}),
    )
    .await;

    let (result, output) = w.verify().await;

    assert!(result.is_err());
    assert!(output.contains("structures/forbidden"), "{output}");
}
