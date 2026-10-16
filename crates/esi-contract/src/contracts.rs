//! The contracts: what the app sends to and reads from ESI and EVE SSO.
//!
//! These are hypotheses until `cargo run -p esi-verify` has held them against real EVE.

use crate::model::{
    BodySpec, Call, Field, HeaderFormat, HeaderSpec, Presence, Route, SelectValue, StatusSpec, Ty,
    Variant, Verifiable,
};

use HeaderFormat::{HttpDate, Integer, Token};
use Presence::{Absent, Always, Optional};

fn field(name: &'static str, ty: Ty, presence: Presence) -> Field {
    Field::new(name, ty, presence)
}

fn header(name: &'static str, format: HeaderFormat, presence: Presence) -> HeaderSpec {
    HeaderSpec::new(name, format, presence)
}

fn variant(
    name: &'static str,
    status: StatusSpec,
    headers: Vec<HeaderSpec>,
    body: BodySpec,
) -> Variant {
    Variant {
        name,
        status,
        headers,
        body,
        verifiable: Verifiable::Yes,
    }
}

fn unverifiable(mut variant: Variant, reason: &'static str) -> Variant {
    variant.verifiable = Verifiable::No(reason);
    variant
}

fn json_object(fields: Vec<Field>) -> BodySpec {
    BodySpec::Json(Ty::Object(fields))
}

/// The headers the app's gate reads on every ESI answer, success or error.
fn error_limit_headers() -> Vec<HeaderSpec> {
    vec![
        header("x-esi-error-limit-remain", Integer, Always),
        header("x-esi-error-limit-reset", Integer, Always),
    ]
}

/// What the app reads from a cacheable ESI success: expiry, clock and the limits.
fn esi_success_headers() -> Vec<HeaderSpec> {
    let mut headers = vec![
        header("date", HttpDate, Always),
        header("expires", HttpDate, Always),
    ];
    headers.extend(error_limit_headers());
    headers.extend([
        // Real ESI sends no rate-limit headers on these routes (verified); only the 429 may.
        header("x-ratelimit-group", Token, Absent),
        header("x-ratelimit-remaining", Integer, Absent),
    ]);
    headers
}

/// What the app reads from an ESI error that real ESI answers to a correct request.
fn esi_error_headers() -> Vec<HeaderSpec> {
    let mut headers = vec![header("date", HttpDate, Always)];
    headers.extend(error_limit_headers());
    headers
}

fn error_body() -> BodySpec {
    json_object(vec![field("error", Ty::Str, Always)])
}

/// 420, 429 and 5xx: they cannot be provoked on real ESI without harm, so only the headers the
/// app reads are listed (their names and formats are verified on the success answers).
fn esi_failure_variants() -> Vec<Variant> {
    vec![
        unverifiable(
            variant(
                "error-limited",
                StatusSpec::Exact(420),
                vec![header("x-esi-error-limit-reset", Integer, Optional)],
                BodySpec::Unchecked,
            ),
            "420 means the error limit is used up; provoking it would block the verification character and the app",
        ),
        unverifiable(
            variant(
                "rate-limited",
                StatusSpec::Exact(429),
                vec![
                    header("retry-after", Integer, Optional),
                    header("x-ratelimit-group", Token, Optional),
                ],
                BodySpec::Unchecked,
            ),
            "429 means the rate limit is used up; provoking it would block the verification character and the app",
        ),
        unverifiable(
            variant(
                "server-error",
                StatusSpec::ServerError,
                vec![],
                BodySpec::Unchecked,
            ),
            "a server error cannot be provoked on demand",
        ),
    ]
}

fn order_fields(buy: bool) -> Vec<Field> {
    vec![
        field("order_id", Ty::Int, Always),
        field("type_id", Ty::Int, Always),
        field("region_id", Ty::Int, Always),
        field("location_id", Ty::Int, Always),
        // ESI leaves the flag out of sell orders.
        field(
            "is_buy_order",
            Ty::Bool,
            if buy { Always } else { Optional },
        ),
        field("is_corporation", Ty::Bool, Optional),
        field("price", Ty::Number, Always),
        field("volume_remain", Ty::Int, Always),
        field("volume_total", Ty::Int, Always),
        field("range", Ty::Str, Always),
        field("issued", Ty::DateTime, Always),
        field("duration", Ty::Int, Always),
        // ESI only sends a minimum and an escrow for buy orders.
        field("min_volume", Ty::Int, if buy { Always } else { Absent }),
        field("escrow", Ty::Number, if buy { Optional } else { Absent }),
    ]
}

pub fn authorization() -> Call {
    let query = |names: &[&'static str]| {
        json_object(names.iter().map(|n| field(n, Ty::Str, Always)).collect())
    };
    Call {
        name: "authorization",
        routes: vec![
            Route {
                method: "GET",
                path: "/v2/oauth/authorize",
            },
            Route {
                method: "GET",
                path: "/v2/oauth/authorize/select",
            },
        ],
        variants: vec![
            // The query of the authorization URL the app builds.
            variant(
                "request",
                StatusSpec::NotHttp,
                vec![],
                query(&[
                    "response_type",
                    "redirect_uri",
                    "client_id",
                    "scope",
                    "code_challenge",
                    "code_challenge_method",
                    "state",
                ]),
            ),
            // The query of the request that reaches the app's callback after the login.
            variant(
                "callback",
                StatusSpec::NotHttp,
                vec![],
                query(&["code", "state"]),
            ),
        ],
    }
}

pub fn token() -> Call {
    let token_body = |refresh: Presence| {
        json_object(vec![
            field("access_token", Ty::Str, Always),
            field("token_type", Ty::Str, Always),
            field("expires_in", Ty::Int, Always),
            field("refresh_token", Ty::Str, refresh),
        ])
    };
    Call {
        name: "token",
        routes: vec![Route {
            method: "POST",
            path: "/v2/oauth/token",
        }],
        variants: vec![
            variant(
                "authorization-code",
                StatusSpec::Exact(200),
                vec![],
                token_body(Always),
            ),
            variant(
                "refresh",
                StatusSpec::Exact(200),
                vec![],
                // The app keeps its old refresh token when SSO sends none.
                token_body(Optional),
            ),
            // The claims inside the access token's payload.
            variant(
                "access-token-claims",
                StatusSpec::NotHttp,
                vec![],
                json_object(vec![
                    field("iss", Ty::Str, Always),
                    field("sub", Ty::Str, Always),
                    field("aud", Ty::StringOrStrings, Always),
                    field("name", Ty::Str, Always),
                    field("owner", Ty::Str, Always),
                    field("exp", Ty::Int, Always),
                    field("scp", Ty::StringOrStrings, Optional),
                ]),
            ),
        ],
    }
}

pub fn jwks() -> Call {
    Call {
        name: "jwks",
        routes: vec![Route {
            method: "GET",
            path: "/oauth/jwks",
        }],
        variants: vec![variant(
            "rsa-keys",
            StatusSpec::Exact(200),
            vec![],
            json_object(vec![field(
                "keys",
                // EVE also publishes keys of other types; the app only uses the RSA ones.
                Ty::array_where(
                    "kty",
                    SelectValue::Str("RSA"),
                    Ty::Object(vec![
                        field("kty", Ty::Str, Always),
                        field("kid", Ty::Str, Always),
                        field("n", Ty::Str, Always),
                        field("e", Ty::Str, Always),
                    ]),
                ),
                Always,
            )]),
        )],
    }
}

pub fn orders() -> Call {
    let body = |buy: bool| {
        BodySpec::Json(Ty::array_where(
            "is_buy_order",
            if buy {
                SelectValue::Bool(true)
            } else {
                SelectValue::FalseOrMissing
            },
            Ty::Object(order_fields(buy)),
        ))
    };
    let mut variants = vec![
        variant(
            "sell-orders",
            StatusSpec::Exact(200),
            esi_success_headers(),
            body(false),
        ),
        variant(
            "buy-orders",
            StatusSpec::Exact(200),
            esi_success_headers(),
            body(true),
        ),
    ];
    variants.extend(esi_failure_variants());
    Call {
        name: "orders",
        routes: vec![Route {
            method: "GET",
            path: "/characters/{id}/orders",
        }],
        variants,
    }
}

pub fn names() -> Call {
    let mut variants = vec![
        variant(
            "found",
            StatusSpec::Exact(200),
            // A POST is not cached: real ESI sends no `Expires`.
            esi_success_headers()
                .into_iter()
                .map(|mut h| {
                    if h.name == "expires" {
                        h.presence = Optional;
                    }
                    h
                })
                .collect(),
            BodySpec::Json(Ty::array_of(Ty::Object(vec![
                field("category", Ty::Str, Always),
                field("id", Ty::Int, Always),
                field("name", Ty::Str, Always),
            ]))),
        ),
        // One unknown ID fails the whole request.
        variant(
            "unknown-id",
            StatusSpec::Exact(404),
            esi_error_headers(),
            error_body(),
        ),
    ];
    variants.extend(esi_failure_variants());
    Call {
        name: "names",
        routes: vec![Route {
            method: "POST",
            path: "/universe/names",
        }],
        variants,
    }
}

pub fn structures() -> Call {
    let mut variants = vec![
        variant(
            "found",
            StatusSpec::Exact(200),
            esi_success_headers(),
            json_object(vec![field("name", Ty::Str, Always)]),
        ),
        variant(
            "forbidden",
            StatusSpec::Exact(403),
            esi_error_headers(),
            error_body(),
        ),
    ];
    variants.extend(esi_failure_variants());
    Call {
        name: "structures",
        routes: vec![Route {
            method: "GET",
            path: "/universe/structures/{id}",
        }],
        variants,
    }
}

/// Every contract.
pub fn contracts() -> Vec<Call> {
    vec![
        authorization(),
        token(),
        jwks(),
        orders(),
        names(),
        structures(),
    ]
}

/// The call with this name. Panics for an unknown name: it is a typo in code.
pub fn contract(name: &str) -> Call {
    contracts()
        .into_iter()
        .find(|c| c.name == name)
        .unwrap_or_else(|| panic!("no contract named {name}"))
}

/// Where routes and contracts do not match: a route without a contract, or a contract that names a
/// route that does not exist. Empty when they agree.
pub fn coverage(stub_routes: &[(&str, &str)]) -> Vec<String> {
    let all = contracts();
    let contracted: Vec<(&str, &str)> = all
        .iter()
        .flat_map(|c| c.routes.iter().map(|r| (r.method, r.path)))
        .collect();
    let mut problems = Vec::new();
    for (method, path) in stub_routes {
        if !contracted.contains(&(*method, *path)) {
            problems.push(format!("stub route {method} {path} has no contract"));
        }
    }
    for call in &all {
        for route in &call.routes {
            if !stub_routes.contains(&(route.method, route.path)) {
                problems.push(format!(
                    "contract {} names {} {}, which the stub does not serve",
                    call.name, route.method, route.path
                ));
            }
        }
    }
    problems
}

/// Canonical JSON of all contracts, for the fingerprint.
pub fn canonical_json() -> String {
    serde_json::to_string(&contracts()).expect("contracts serialize")
}

#[cfg(test)]
mod tests {
    use http::{HeaderMap, HeaderName, HeaderValue};
    use serde_json::{Value, json};

    use super::*;
    use crate::check::{ViolationKind, check};

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.insert(
                HeaderName::try_from(*name).unwrap(),
                HeaderValue::from_str(value).unwrap(),
            );
        }
        map
    }

    fn esi_ok() -> HeaderMap {
        headers(&[
            ("date", "Fri, 16 Oct 2026 10:00:00 GMT"),
            ("expires", "Fri, 16 Oct 2026 10:20:00 GMT"),
            ("x-esi-error-limit-remain", "100"),
            ("x-esi-error-limit-reset", "60"),
        ])
    }

    fn esi_error() -> HeaderMap {
        headers(&[
            ("date", "Fri, 16 Oct 2026 10:00:00 GMT"),
            ("x-esi-error-limit-remain", "99"),
            ("x-esi-error-limit-reset", "59"),
        ])
    }

    fn problems(
        call: &Call,
        variant: &str,
        status: Option<u16>,
        h: &HeaderMap,
        body: Option<Value>,
    ) -> Vec<String> {
        check(call, variant, status, h, body.as_ref())
            .iter()
            .map(|v| format!("{} {}", v.at, v.problem))
            .collect()
    }

    fn sell() -> Value {
        json!({
            "order_id": 1, "type_id": 34, "region_id": 10000002, "location_id": 60003760,
            "is_corporation": false, "price": 10.5,
            "volume_remain": 1, "volume_total": 2, "range": "station",
            "issued": "2026-10-11T12:00:00Z", "duration": 90,
        })
    }

    fn buy() -> Value {
        let mut order = sell();
        order["is_buy_order"] = json!(true);
        order["min_volume"] = json!(1);
        order["escrow"] = json!(5.0);
        order
    }

    #[test]
    fn every_call_has_a_route_and_unique_names() {
        let all = contracts();
        let mut names: Vec<_> = all.iter().map(|c| c.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), all.len());
        assert!(all.iter().all(|c| !c.routes.is_empty()));
        assert!(
            all.iter().all(|c| {
                let mut v: Vec<_> = c.variants.iter().map(|v| v.name).collect();
                v.sort_unstable();
                v.dedup();
                v.len() == c.variants.len()
            }),
            "variant names are unique within a call"
        );
    }

    #[test]
    fn the_failure_variants_are_not_verifiable_and_say_why() {
        for call in [orders(), names(), structures()] {
            for name in ["error-limited", "rate-limited", "server-error"] {
                let v = call.variant(name).unwrap();
                assert!(
                    matches!(v.verifiable, Verifiable::No(reason) if !reason.is_empty()),
                    "{}/{name}",
                    call.name
                );
            }
        }
    }

    #[test]
    fn authorization_request_and_callback() {
        let call = authorization();
        let request = json!({
            "response_type": "code", "redirect_uri": "https://x/cb", "client_id": "c",
            "scope": "a b", "code_challenge": "x", "code_challenge_method": "S256", "state": "s",
        });
        assert!(
            problems(
                &call,
                "request",
                None,
                &HeaderMap::new(),
                Some(request.clone())
            )
            .is_empty()
        );
        let mut broken = request;
        broken.as_object_mut().unwrap().remove("state");
        assert_eq!(
            problems(&call, "request", None, &HeaderMap::new(), Some(broken)),
            ["$.state is missing but always present"]
        );
        assert!(
            problems(
                &call,
                "callback",
                None,
                &HeaderMap::new(),
                Some(json!({"code": "c", "state": "s"}))
            )
            .is_empty()
        );
        assert_eq!(
            problems(
                &call,
                "callback",
                None,
                &HeaderMap::new(),
                Some(json!({"code": "c"}))
            ),
            ["$.state is missing but always present"]
        );
    }

    #[test]
    fn token_grants_and_claims() {
        let call = token();
        let full = json!({"access_token": "a", "token_type": "Bearer", "expires_in": 1199, "refresh_token": "r"});
        for variant in ["authorization-code", "refresh"] {
            assert!(
                problems(
                    &call,
                    variant,
                    Some(200),
                    &HeaderMap::new(),
                    Some(full.clone())
                )
                .is_empty()
            );
        }
        let mut no_refresh = full;
        no_refresh.as_object_mut().unwrap().remove("refresh_token");
        assert_eq!(
            problems(
                &call,
                "authorization-code",
                Some(200),
                &HeaderMap::new(),
                Some(no_refresh.clone())
            ),
            ["$.refresh_token is missing but always present"]
        );
        assert!(
            problems(
                &call,
                "refresh",
                Some(200),
                &HeaderMap::new(),
                Some(no_refresh)
            )
            .is_empty()
        );

        let claims = json!({
            "iss": "https://login.eveonline.com", "sub": "CHARACTER:EVE:1", "aud": ["c", "EVE Online"],
            "name": "N", "owner": "o", "exp": 1, "scp": "one-scope",
        });
        assert!(
            problems(
                &call,
                "access-token-claims",
                None,
                &HeaderMap::new(),
                Some(claims.clone())
            )
            .is_empty()
        );
        let mut broken = claims;
        broken["exp"] = json!("soon");
        assert_eq!(
            problems(
                &call,
                "access-token-claims",
                None,
                &HeaderMap::new(),
                Some(broken)
            ),
            ["$.exp expected integer, found string"]
        );
    }

    #[test]
    fn jwks_checks_only_rsa_keys() {
        let call = jwks();
        let rsa = json!({"kty": "RSA", "kid": "k", "n": "n", "e": "e", "alg": "RS256"});
        let ec = json!({"kty": "EC", "kid": "k2", "crv": "P-256"});
        assert!(
            problems(
                &call,
                "rsa-keys",
                Some(200),
                &HeaderMap::new(),
                Some(json!({"keys": [rsa.clone(), ec.clone()]}))
            )
            .is_empty()
        );
        let mut no_n = rsa;
        no_n.as_object_mut().unwrap().remove("n");
        assert_eq!(
            problems(
                &call,
                "rsa-keys",
                Some(200),
                &HeaderMap::new(),
                Some(json!({"keys": [ec, no_n]}))
            ),
            ["$.keys[1].n is missing but always present"]
        );
    }

    #[test]
    fn sell_and_buy_orders() {
        let call = orders();
        let body = json!([sell(), buy()]);
        for variant in ["sell-orders", "buy-orders"] {
            assert!(
                problems(&call, variant, Some(200), &esi_ok(), Some(body.clone())).is_empty(),
                "{variant}"
            );
        }

        // The case the stub once got wrong: a minimum on a sell order.
        let mut sell_with_min = sell();
        sell_with_min["min_volume"] = json!(1);
        assert_eq!(
            problems(
                &call,
                "sell-orders",
                Some(200),
                &esi_ok(),
                Some(json!([sell_with_min]))
            ),
            ["$[0].min_volume is present but must be absent"]
        );
        // ... and a buy order without one.
        let mut buy_without_min = buy();
        buy_without_min
            .as_object_mut()
            .unwrap()
            .remove("min_volume");
        assert_eq!(
            problems(
                &call,
                "buy-orders",
                Some(200),
                &esi_ok(),
                Some(json!([buy_without_min]))
            ),
            ["$[0].min_volume is missing but always present"]
        );
        // Only sell orders: nothing to verify on the buy side.
        let only_sell = check(
            &call,
            "buy-orders",
            Some(200),
            &esi_ok(),
            Some(&json!([sell()])),
        );
        assert_eq!(only_sell.len(), 1);
        assert_eq!(only_sell[0].kind, ViolationKind::NothingToCheck);
    }

    #[test]
    fn a_sell_order_is_selected_with_the_flag_missing_or_false() {
        let call = orders();
        let mut explicit = sell();
        explicit["is_buy_order"] = json!(false);
        // Real ESI leaves the flag out; an explicit false is still a sell order.
        assert!(
            problems(
                &call,
                "sell-orders",
                Some(200),
                &esi_ok(),
                Some(json!([sell()]))
            )
            .is_empty()
        );
        assert!(
            problems(
                &call,
                "sell-orders",
                Some(200),
                &esi_ok(),
                Some(json!([explicit]))
            )
            .is_empty()
        );
        // A buy order must say so.
        let mut unflagged_buy = buy();
        unflagged_buy
            .as_object_mut()
            .unwrap()
            .remove("is_buy_order");
        let found = check(
            &call,
            "buy-orders",
            Some(200),
            &esi_ok(),
            Some(&json!([unflagged_buy])),
        );
        assert_eq!(found[0].kind, ViolationKind::NothingToCheck);
    }

    #[test]
    fn names_need_no_expiry_because_a_post_is_not_cached() {
        let mut h = esi_ok();
        h.remove("expires");
        let found = json!([{"category": "region", "id": 1, "name": "x"}]);
        assert!(problems(&names(), "found", Some(200), &h, Some(found)).is_empty());
    }

    #[test]
    fn orders_need_the_headers_the_app_reads() {
        let call = orders();
        let mut h = esi_ok();
        h.remove("expires");
        assert_eq!(
            problems(&call, "sell-orders", Some(200), &h, Some(json!([sell()]))),
            ["header expires is missing but always present"]
        );
    }

    #[test]
    fn names_found_and_unknown() {
        let call = names();
        let found = json!([{"category": "region", "id": 10000002, "name": "The Forge"}]);
        assert!(problems(&call, "found", Some(200), &esi_ok(), Some(found)).is_empty());
        assert_eq!(
            problems(
                &call,
                "found",
                Some(200),
                &esi_ok(),
                Some(json!([{"category": "region", "id": 1}]))
            ),
            ["$[0].name is missing but always present"]
        );
        assert!(
            problems(
                &call,
                "unknown-id",
                Some(404),
                &esi_error(),
                Some(json!({"error": "Invalid IDs in the request"}))
            )
            .is_empty()
        );
        assert_eq!(
            problems(
                &call,
                "unknown-id",
                Some(404),
                &esi_error(),
                Some(json!({}))
            ),
            ["$.error is missing but always present"]
        );
    }

    #[test]
    fn structures_found_and_forbidden() {
        let call = structures();
        assert!(
            problems(
                &call,
                "found",
                Some(200),
                &esi_ok(),
                Some(json!({"name": "Keepstar"}))
            )
            .is_empty()
        );
        assert_eq!(
            problems(
                &call,
                "found",
                Some(200),
                &esi_ok(),
                Some(json!({"owner_id": 1}))
            ),
            ["$.name is missing but always present"]
        );
        assert!(
            problems(
                &call,
                "forbidden",
                Some(403),
                &esi_error(),
                Some(json!({"error": "Forbidden"}))
            )
            .is_empty()
        );
        assert_eq!(
            problems(
                &call,
                "forbidden",
                Some(403),
                &HeaderMap::new(),
                Some(json!({"error": "Forbidden"}))
            )
            .len(),
            3
        );
    }

    #[test]
    fn failure_variants_list_only_what_the_app_reads() {
        let call = orders();
        assert!(
            problems(
                &call,
                "error-limited",
                Some(420),
                &headers(&[("x-esi-error-limit-reset", "30")]),
                None
            )
            .is_empty()
        );
        assert!(problems(&call, "error-limited", Some(420), &HeaderMap::new(), None).is_empty());
        assert!(
            problems(
                &call,
                "rate-limited",
                Some(429),
                &headers(&[("retry-after", "60"), ("x-ratelimit-group", "char-market")]),
                None
            )
            .is_empty()
        );
        assert_eq!(
            problems(
                &call,
                "rate-limited",
                Some(429),
                &headers(&[("retry-after", "soon")]),
                None
            )
            .len(),
            1
        );
        assert!(problems(&call, "server-error", Some(502), &HeaderMap::new(), None).is_empty());
    }

    const ALL_ROUTES: [(&str, &str); 7] = [
        ("GET", "/oauth/jwks"),
        ("GET", "/v2/oauth/authorize"),
        ("GET", "/v2/oauth/authorize/select"),
        ("POST", "/v2/oauth/token"),
        ("GET", "/characters/{id}/orders"),
        ("POST", "/universe/names"),
        ("GET", "/universe/structures/{id}"),
    ];

    #[test]
    fn coverage_is_empty_when_routes_and_contracts_agree() {
        assert_eq!(coverage(&ALL_ROUTES), Vec::<String>::new());
    }

    #[test]
    fn a_route_without_a_contract_is_reported_by_name() {
        let mut routes = ALL_ROUTES.to_vec();
        routes.push(("GET", "/characters/{id}/wallet"));
        assert_eq!(
            coverage(&routes),
            ["stub route GET /characters/{id}/wallet has no contract"]
        );
    }

    #[test]
    fn a_contract_without_a_route_is_reported() {
        let problems = coverage(&ALL_ROUTES[1..]);
        assert_eq!(problems.len(), 1);
        assert!(problems[0].contains("/oauth/jwks"), "{problems:?}");
    }

    #[test]
    fn contracts_serialize_deterministically() {
        assert_eq!(canonical_json(), canonical_json());
        assert!(canonical_json().contains("\"sell-orders\""));
    }
}
