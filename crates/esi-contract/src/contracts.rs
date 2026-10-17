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

/// The public market orders of a region for one item. No token; every page carries `X-Pages`.
pub fn market_orders() -> Call {
    // Real ESI limits this route by rate, not by error count (verified): it sends the rate-limit
    // group and what is left of it, and no error-limit headers.
    let headers = vec![
        header("date", HttpDate, Always),
        header("expires", HttpDate, Always),
        header("x-pages", Integer, Always),
        header("x-ratelimit-group", Token, Always),
        header("x-ratelimit-remaining", Integer, Always),
        header("x-esi-error-limit-remain", Integer, Absent),
        header("x-esi-error-limit-reset", Integer, Absent),
    ];
    // Only what the app reads: not the volumes, duration or issue date.
    let fields = vec![
        field("order_id", Ty::Int, Always),
        field("type_id", Ty::Int, Always),
        field("location_id", Ty::Int, Always),
        field("system_id", Ty::Int, Always),
        field("is_buy_order", Ty::Bool, Always),
        field("price", Ty::Number, Always),
        field("range", Ty::Str, Always),
    ];
    let mut variants = vec![variant(
        "orders",
        StatusSpec::Exact(200),
        headers,
        BodySpec::Json(Ty::array_of(Ty::Object(fields))),
    )];
    variants.extend(esi_failure_variants());
    Call {
        name: "market-orders",
        routes: vec![Route {
            method: "GET",
            path: "/markets/{region_id}/orders",
        }],
        variants,
    }
}

/// What real ESI sends on the character wallet and asset routes (verified): the rate-limit group
/// of the route and what is left of it, and no error-limit headers. A success is cacheable.
fn char_route_headers(success: bool) -> Vec<HeaderSpec> {
    let mut headers = vec![header("date", HttpDate, Always)];
    if success {
        headers.push(header("expires", HttpDate, Always));
    }
    headers.extend([
        header("x-ratelimit-group", Token, Always),
        header("x-ratelimit-remaining", Integer, Always),
        header("x-esi-error-limit-remain", Integer, Absent),
        header("x-esi-error-limit-reset", Integer, Absent),
    ]);
    headers
}

/// The wallet balance of a character: a bare JSON number.
pub fn wallet() -> Call {
    let mut variants = vec![
        variant(
            "balance",
            StatusSpec::Exact(200),
            char_route_headers(true),
            BodySpec::Json(Ty::Number),
        ),
        variant(
            "forbidden",
            StatusSpec::Exact(403),
            char_route_headers(false),
            error_body(),
        ),
    ];
    variants.extend(esi_failure_variants());
    Call {
        name: "wallet",
        routes: vec![Route {
            method: "GET",
            path: "/characters/{id}/wallet",
        }],
        variants,
    }
}

/// The assets of a character, one page per call; every page carries `X-Pages`.
pub fn assets() -> Call {
    let mut headers = char_route_headers(true);
    headers.push(header("x-pages", Integer, Always));
    // Only what the app reads. ESI leaves the blueprint flag out of everything else.
    let fields = vec![
        field("item_id", Ty::Int, Always),
        field("type_id", Ty::Int, Always),
        field("location_id", Ty::Int, Always),
        field("location_type", Ty::Str, Always),
        field("location_flag", Ty::Str, Always),
        field("quantity", Ty::Int, Always),
        field("is_singleton", Ty::Bool, Always),
        field("is_blueprint_copy", Ty::Bool, Optional),
    ];
    let mut variants = vec![
        variant(
            "items",
            StatusSpec::Exact(200),
            headers,
            BodySpec::Json(Ty::array_of(Ty::Object(fields))),
        ),
        variant(
            "forbidden",
            StatusSpec::Exact(403),
            char_route_headers(false),
            error_body(),
        ),
    ];
    variants.extend(esi_failure_variants());
    Call {
        name: "assets",
        routes: vec![Route {
            method: "GET",
            path: "/characters/{id}/assets",
        }],
        variants,
    }
}

/// The skills of a character. Only the active level is read: it is the one that applies.
pub fn skills() -> Call {
    let mut variants = vec![
        variant(
            "levels",
            StatusSpec::Exact(200),
            char_route_headers(true),
            json_object(vec![field(
                "skills",
                Ty::array_of(Ty::Object(vec![
                    field("skill_id", Ty::Int, Always),
                    field("active_skill_level", Ty::Int, Always),
                ])),
                Always,
            )]),
        ),
        variant(
            "forbidden",
            StatusSpec::Exact(403),
            char_route_headers(false),
            error_body(),
        ),
    ];
    variants.extend(esi_failure_variants());
    Call {
        name: "skills",
        routes: vec![Route {
            method: "GET",
            path: "/characters/{id}/skills",
        }],
        variants,
    }
}

/// The standings of a character towards agents, NPC corporations and factions.
pub fn standings() -> Call {
    let mut variants = vec![
        variant(
            "list",
            StatusSpec::Exact(200),
            char_route_headers(true),
            BodySpec::Json(Ty::array_of(Ty::Object(vec![
                field("from_id", Ty::Int, Always),
                field("from_type", Ty::Str, Always),
                field("standing", Ty::Number, Always),
            ]))),
        ),
        variant(
            "forbidden",
            StatusSpec::Exact(403),
            char_route_headers(false),
            error_body(),
        ),
    ];
    variants.extend(esi_failure_variants());
    Call {
        name: "standings",
        routes: vec![Route {
            method: "GET",
            path: "/characters/{id}/standings",
        }],
        variants,
    }
}

/// An NPC station. Public; only its owner corporation is read.
pub fn stations() -> Call {
    let mut variants = vec![variant(
        "station",
        StatusSpec::Exact(200),
        esi_success_headers(),
        json_object(vec![field("owner", Ty::Int, Always)]),
    )];
    variants.extend(esi_failure_variants());
    Call {
        name: "stations",
        routes: vec![Route {
            method: "GET",
            path: "/universe/stations/{id}",
        }],
        variants,
    }
}

/// ESI's market prices for every item. Public; an item may have no average.
pub fn prices() -> Call {
    let mut variants = vec![variant(
        "list",
        StatusSpec::Exact(200),
        esi_success_headers(),
        // `adjusted_price` is not read.
        BodySpec::Json(Ty::array_of(Ty::Object(vec![
            field("type_id", Ty::Int, Always),
            field("average_price", Ty::Number, Optional),
        ]))),
    )];
    variants.extend(esi_failure_variants());
    Call {
        name: "prices",
        routes: vec![Route {
            method: "GET",
            path: "/markets/prices",
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
        market_orders(),
        wallet(),
        assets(),
        prices(),
        skills(),
        standings(),
        stations(),
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
        for call in [
            orders(),
            names(),
            structures(),
            wallet(),
            assets(),
            prices(),
            skills(),
            standings(),
            stations(),
        ] {
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

    fn market_order() -> Value {
        json!({
            "order_id": 7, "type_id": 34, "location_id": 60003760, "system_id": 30000142,
            "is_buy_order": false, "price": 10.5, "range": "region",
            "duration": 90, "issued": "2026-10-11T12:00:00Z", "min_volume": 1,
            "volume_remain": 3, "volume_total": 5,
        })
    }

    /// What real ESI sends on the market route: the rate-limit headers, no error-limit ones.
    fn market_ok() -> HeaderMap {
        headers(&[
            ("date", "Fri, 16 Oct 2026 10:00:00 GMT"),
            ("expires", "Fri, 16 Oct 2026 10:05:00 GMT"),
            ("x-pages", "3"),
            ("x-ratelimit-group", "market-order"),
            ("x-ratelimit-remaining", "11998"),
        ])
    }

    #[test]
    fn market_orders_answer_with_the_fields_and_pages_the_app_reads() {
        let call = market_orders();
        let body = json!([market_order()]);
        assert!(problems(&call, "orders", Some(200), &market_ok(), Some(body.clone())).is_empty());
        // The app does not read the volumes or the rest, so they are not required.
        let slim = json!([{
            "order_id": 7, "type_id": 34, "location_id": 60003760, "system_id": 30000142,
            "is_buy_order": true, "price": 10.5, "range": "5",
        }]);
        assert!(problems(&call, "orders", Some(200), &market_ok(), Some(slim)).is_empty());
        // An item nobody trades is an empty page.
        assert!(problems(&call, "orders", Some(200), &market_ok(), Some(json!([]))).is_empty());

        let mut h = market_ok();
        h.remove("x-pages");
        assert_eq!(
            problems(&call, "orders", Some(200), &h, Some(body.clone())),
            ["header x-pages is missing but always present"]
        );
        h.insert("x-pages", HeaderValue::from_static("many"));
        assert_eq!(
            problems(&call, "orders", Some(200), &h, Some(body)).len(),
            1
        );
    }

    #[test]
    fn the_market_route_is_rate_limited_not_error_limited() {
        let call = market_orders();
        let body = json!([market_order()]);
        let mut h = market_ok();
        h.remove("x-ratelimit-group");
        assert_eq!(
            problems(&call, "orders", Some(200), &h, Some(body.clone())),
            ["header x-ratelimit-group is missing but always present"]
        );
        // The error limit headers are not sent here.
        let mut h = market_ok();
        h.insert("x-esi-error-limit-remain", HeaderValue::from_static("100"));
        assert_eq!(
            problems(&call, "orders", Some(200), &h, Some(body)),
            ["header x-esi-error-limit-remain is present but must be absent"]
        );
    }

    #[test]
    fn a_market_order_without_a_system_breaks_the_contract() {
        for field in [
            "system_id",
            "order_id",
            "type_id",
            "location_id",
            "is_buy_order",
            "price",
            "range",
        ] {
            let mut order = market_order();
            order.as_object_mut().unwrap().remove(field);
            assert_eq!(
                problems(
                    &market_orders(),
                    "orders",
                    Some(200),
                    &market_ok(),
                    Some(json!([order]))
                ),
                [format!("$[0].{field} is missing but always present")]
            );
        }
    }

    #[test]
    fn market_orders_need_no_token_and_have_the_usual_failures() {
        let call = market_orders();
        for name in ["error-limited", "rate-limited", "server-error"] {
            assert!(
                matches!(
                    call.variant(name).map(|v| v.verifiable),
                    Some(Verifiable::No(_))
                ),
                "{name}"
            );
        }
        assert!(problems(&call, "server-error", Some(503), &HeaderMap::new(), None).is_empty());
        assert_eq!(call.routes[0].path, "/markets/{region_id}/orders");
    }

    fn asset() -> Value {
        json!({
            "item_id": 1000000000001_i64, "type_id": 34, "location_id": 60003760,
            "location_type": "station", "location_flag": "Hangar", "quantity": 100,
            "is_singleton": false,
        })
    }

    /// What real ESI sends on the character wallet and asset routes: a rate-limit group of the
    /// route, no error-limit headers (verified).
    fn char_ok(group: &str) -> HeaderMap {
        headers(&[
            ("date", "Fri, 16 Oct 2026 10:00:00 GMT"),
            ("expires", "Fri, 16 Oct 2026 10:20:00 GMT"),
            ("x-ratelimit-group", group),
            ("x-ratelimit-remaining", "148"),
        ])
    }

    fn char_forbidden(group: &str) -> HeaderMap {
        headers(&[
            ("date", "Fri, 16 Oct 2026 10:00:00 GMT"),
            ("x-ratelimit-group", group),
            ("x-ratelimit-remaining", "143"),
        ])
    }

    fn assets_ok() -> HeaderMap {
        let mut h = char_ok("char-asset");
        h.insert("x-pages", HeaderValue::from_static("2"));
        h
    }

    #[test]
    fn wallet_and_assets_are_rate_limited_not_error_limited() {
        let balance = json!(1.0);
        let mut h = char_ok("char-wallet");
        h.insert("x-esi-error-limit-remain", HeaderValue::from_static("100"));
        assert_eq!(
            problems(&wallet(), "balance", Some(200), &h, Some(balance.clone())),
            ["header x-esi-error-limit-remain is present but must be absent"]
        );
        let mut h = char_ok("char-wallet");
        h.remove("x-ratelimit-group");
        assert_eq!(
            problems(&wallet(), "balance", Some(200), &h, Some(balance)),
            ["header x-ratelimit-group is missing but always present"]
        );
        let mut h = char_forbidden("char-asset");
        h.insert("x-esi-error-limit-reset", HeaderValue::from_static("60"));
        assert_eq!(
            problems(
                &assets(),
                "forbidden",
                Some(403),
                &h,
                Some(json!({"error": "Forbidden"}))
            ),
            ["header x-esi-error-limit-reset is present but must be absent"]
        );
    }

    #[test]
    fn the_wallet_is_a_bare_number() {
        let call = wallet();
        assert!(
            problems(
                &call,
                "balance",
                Some(200),
                &char_ok("char-wallet"),
                Some(json!(1234.56))
            )
            .is_empty()
        );
        assert!(
            problems(
                &call,
                "balance",
                Some(200),
                &char_ok("char-wallet"),
                Some(json!(0))
            )
            .is_empty()
        );
        assert_eq!(
            problems(
                &call,
                "balance",
                Some(200),
                &char_ok("char-wallet"),
                Some(json!({"balance": 1}))
            ),
            ["$ expected number, found object"]
        );
        assert!(
            problems(
                &call,
                "forbidden",
                Some(403),
                &char_forbidden("char-wallet"),
                Some(json!({"error": "Forbidden"}))
            )
            .is_empty()
        );
        assert_eq!(call.routes[0].path, "/characters/{id}/wallet");
    }

    #[test]
    fn assets_answer_with_the_fields_and_pages_the_app_reads() {
        let call = assets();
        let mut copy = asset();
        copy["is_blueprint_copy"] = json!(true);
        let body = json!([asset(), copy]);
        assert!(problems(&call, "items", Some(200), &assets_ok(), Some(body.clone())).is_empty());

        let mut h = assets_ok();
        h.remove("x-pages");
        assert_eq!(
            problems(&call, "items", Some(200), &h, Some(body)),
            ["header x-pages is missing but always present"]
        );
        for field in [
            "item_id",
            "type_id",
            "location_id",
            "location_type",
            "location_flag",
            "quantity",
            "is_singleton",
        ] {
            let mut item = asset();
            item.as_object_mut().unwrap().remove(field);
            assert_eq!(
                problems(&call, "items", Some(200), &assets_ok(), Some(json!([item]))),
                [format!("$[0].{field} is missing but always present")]
            );
        }
        assert!(
            problems(
                &call,
                "forbidden",
                Some(403),
                &char_forbidden("char-asset"),
                Some(json!({"error": "Forbidden"}))
            )
            .is_empty()
        );
        assert_eq!(call.routes[0].path, "/characters/{id}/assets");
    }

    #[test]
    fn skills_carry_the_active_level() {
        let call = skills();
        let body = json!({"skills": [
            {"skill_id": 16622, "active_skill_level": 4, "trained_skill_level": 5, "skillpoints_in_skill": 1},
        ], "total_sp": 1});
        assert!(
            problems(
                &call,
                "levels",
                Some(200),
                &char_ok("char-skills"),
                Some(body)
            )
            .is_empty()
        );
        let broken = json!({"skills": [{"skill_id": 16622, "trained_skill_level": 5}]});
        assert_eq!(
            problems(
                &call,
                "levels",
                Some(200),
                &char_ok("char-skills"),
                Some(broken)
            ),
            ["$.skills[0].active_skill_level is missing but always present"]
        );
        assert!(
            problems(
                &call,
                "forbidden",
                Some(403),
                &char_forbidden("char-skills"),
                Some(json!({"error": "forbidden"}))
            )
            .is_empty()
        );
        assert_eq!(call.routes[0].path, "/characters/{id}/skills");
    }

    #[test]
    fn standings_carry_kind_and_value() {
        let call = standings();
        let body = json!([{"from_id": 1000035, "from_type": "npc_corp", "standing": 1.5}]);
        assert!(
            problems(
                &call,
                "list",
                Some(200),
                &char_ok("char-standings"),
                Some(body)
            )
            .is_empty()
        );
        let broken = json!([{"from_id": 1000035, "from_type": "npc_corp"}]);
        assert_eq!(
            problems(
                &call,
                "list",
                Some(200),
                &char_ok("char-standings"),
                Some(broken)
            ),
            ["$[0].standing is missing but always present"]
        );
        assert_eq!(call.routes[0].path, "/characters/{id}/standings");
    }

    #[test]
    fn a_station_needs_its_owner() {
        let call = stations();
        let body = json!({"station_id": 60003760, "name": "Jita", "owner": 1000035});
        assert!(problems(&call, "station", Some(200), &esi_ok(), Some(body)).is_empty());
        assert_eq!(
            problems(
                &call,
                "station",
                Some(200),
                &esi_ok(),
                Some(json!({"name": "x"}))
            ),
            ["$.owner is missing but always present"]
        );
        assert_eq!(call.routes[0].path, "/universe/stations/{id}");
    }

    #[test]
    fn prices_may_lack_an_average_and_need_no_token() {
        let call = prices();
        let body = json!([
            {"type_id": 34, "average_price": 5.5, "adjusted_price": 5.1},
            {"type_id": 99, "adjusted_price": 1.0},
        ]);
        assert!(problems(&call, "list", Some(200), &esi_ok(), Some(body)).is_empty());
        assert_eq!(
            problems(
                &call,
                "list",
                Some(200),
                &esi_ok(),
                Some(json!([{"average_price": 1.0}]))
            ),
            ["$[0].type_id is missing but always present"]
        );
        assert_eq!(call.routes[0].path, "/markets/prices");
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

    const ALL_ROUTES: [(&str, &str); 14] = [
        ("GET", "/oauth/jwks"),
        ("GET", "/v2/oauth/authorize"),
        ("GET", "/v2/oauth/authorize/select"),
        ("POST", "/v2/oauth/token"),
        ("GET", "/characters/{id}/orders"),
        ("POST", "/universe/names"),
        ("GET", "/universe/structures/{id}"),
        ("GET", "/markets/{region_id}/orders"),
        ("GET", "/characters/{id}/wallet"),
        ("GET", "/characters/{id}/assets"),
        ("GET", "/characters/{id}/skills"),
        ("GET", "/characters/{id}/standings"),
        ("GET", "/universe/stations/{id}"),
        ("GET", "/markets/prices"),
    ];

    #[test]
    fn coverage_is_empty_when_routes_and_contracts_agree() {
        assert_eq!(coverage(&ALL_ROUTES), Vec::<String>::new());
    }

    #[test]
    fn a_route_without_a_contract_is_reported_by_name() {
        let mut routes = ALL_ROUTES.to_vec();
        routes.push(("GET", "/characters/{id}/journal"));
        assert_eq!(
            coverage(&routes),
            ["stub route GET /characters/{id}/journal has no contract"]
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
