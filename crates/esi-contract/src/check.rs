//! Checking an observed answer against a contract variant.
//!
//! A violation names the call, the variant and a header or JSON path, and says what is wrong in
//! terms of presence and types. It never contains a value from the answer.

use http::HeaderMap;
use serde_json::Value;

use crate::model::{
    BodySpec, Call, Field, HeaderFormat, HeaderSpec, Presence, Select, SelectValue, StatusSpec, Ty,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViolationKind {
    Status,
    Header,
    Body,
    /// A variant selects elements (for example buy orders) and the answer has none.
    NothingToCheck,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    pub call: String,
    pub variant: String,
    pub kind: ViolationKind,
    /// `status`, `header <name>` or a JSON path like `$[0].price`.
    pub at: String,
    pub problem: String,
}

impl std::fmt::Display for Violation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}/{}: {}: {}",
            self.call, self.variant, self.at, self.problem
        )
    }
}

/// Checks one observed answer. `status` is `None` for things that are not HTTP answers (a request
/// the app builds, the claims of a token).
pub fn check(
    call: &Call,
    variant: &str,
    status: Option<u16>,
    headers: &HeaderMap,
    body: Option<&Value>,
) -> Vec<Violation> {
    let Some(spec) = call.variant(variant) else {
        return vec![Violation {
            call: call.name.to_owned(),
            variant: variant.to_owned(),
            kind: ViolationKind::Status,
            at: "variant".to_owned(),
            problem: "the contract has no such variant".to_owned(),
        }];
    };
    let mut out = Out {
        call: call.name,
        variant: spec.name,
        violations: Vec::new(),
    };

    match (spec.status, status) {
        (StatusSpec::NotHttp, _) => {}
        (StatusSpec::Exact(want), Some(got)) if want == got => {}
        (StatusSpec::ServerError, Some(got)) if (500..600).contains(&got) => {}
        (want, got) => out.push(
            ViolationKind::Status,
            "status".to_owned(),
            format!(
                "expected {}, got {}",
                match want {
                    StatusSpec::NotHttp => "no HTTP status".to_owned(),
                    StatusSpec::Exact(s) => s.to_string(),
                    StatusSpec::ServerError => "5xx".to_owned(),
                },
                got.map_or("none".to_owned(), |s| s.to_string())
            ),
        ),
    }

    for header in &spec.headers {
        check_header(&mut out, header, headers);
    }

    if let BodySpec::Json(ty) = &spec.body {
        match body {
            Some(value) => check_ty(&mut out, "$", ty, value),
            None => out.push(
                ViolationKind::Body,
                "$".to_owned(),
                "there is no JSON body".to_owned(),
            ),
        }
    }
    out.violations
}

struct Out<'a> {
    call: &'a str,
    variant: &'a str,
    violations: Vec<Violation>,
}

impl Out<'_> {
    fn push(&mut self, kind: ViolationKind, at: String, problem: String) {
        self.violations.push(Violation {
            call: self.call.to_owned(),
            variant: self.variant.to_owned(),
            kind,
            at,
            problem,
        });
    }
}

fn check_header(out: &mut Out, spec: &HeaderSpec, headers: &HeaderMap) {
    let at = format!("header {}", spec.name);
    let values = headers.get_all(spec.name);
    let first = values.iter().next();
    match (spec.presence, first) {
        (Presence::Absent, Some(_)) => {
            out.push(
                ViolationKind::Header,
                at,
                "is present but must be absent".to_owned(),
            );
        }
        (Presence::Absent, None) | (Presence::Optional | Presence::Nullable, None) => {}
        (Presence::Always, None) => {
            out.push(
                ViolationKind::Header,
                at,
                "is missing but always present".to_owned(),
            );
        }
        (_, Some(value)) => {
            let Ok(text) = value.to_str() else {
                out.push(
                    ViolationKind::Header,
                    at,
                    "is not visible ASCII text".to_owned(),
                );
                return;
            };
            let ok = match spec.format {
                HeaderFormat::HttpDate => httpdate::parse_http_date(text).is_ok(),
                HeaderFormat::Integer => text.trim().parse::<i64>().is_ok(),
                HeaderFormat::Token => {
                    !text.is_empty() && text.chars().all(|c| c.is_ascii_graphic())
                }
                HeaderFormat::Text => !text.trim().is_empty(),
            };
            if !ok {
                out.push(
                    ViolationKind::Header,
                    at,
                    format!("does not have the format {:?}", spec.format),
                );
            }
        }
    }
}

fn type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(n) if n.is_i64() || n.is_u64() => "integer",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn ty_name(ty: &Ty) -> &'static str {
    match ty {
        Ty::Bool => "boolean",
        Ty::Int => "integer",
        Ty::Number => "number",
        Ty::Str => "string",
        Ty::DateTime => "RFC 3339 date-time string",
        Ty::StringOrStrings => "string or array of strings",
        Ty::Array { .. } => "array",
        Ty::Object(_) => "object",
    }
}

fn selected(select: &Select, element: &Value) -> bool {
    match (element.get(select.field), select.equals) {
        (None | Some(Value::Bool(false)), SelectValue::FalseOrMissing) => true,
        (Some(Value::Bool(b)), SelectValue::Bool(want)) => *b == want,
        (Some(Value::String(s)), SelectValue::Str(want)) => s == want,
        _ => false,
    }
}

fn check_ty(out: &mut Out, at: &str, ty: &Ty, value: &Value) {
    let mismatch = |out: &mut Out| {
        out.push(
            ViolationKind::Body,
            at.to_owned(),
            format!("expected {}, found {}", ty_name(ty), type_name(value)),
        );
    };
    match (ty, value) {
        (Ty::Bool, Value::Bool(_)) => {}
        (Ty::Int, Value::Number(n)) if n.is_i64() || n.is_u64() => {}
        (Ty::Number, Value::Number(_)) => {}
        (Ty::Str, Value::String(_)) => {}
        (Ty::DateTime, Value::String(s)) => {
            if time::OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339)
                .is_err()
            {
                mismatch(out);
            }
        }
        (Ty::StringOrStrings, Value::String(_)) => {}
        (Ty::StringOrStrings, Value::Array(items)) => {
            for (i, item) in items.iter().enumerate() {
                if !item.is_string() {
                    out.push(
                        ViolationKind::Body,
                        format!("{at}[{i}]"),
                        format!("expected string, found {}", type_name(item)),
                    );
                }
            }
        }
        (Ty::Array { of, only }, Value::Array(items)) => {
            let mut checked = 0;
            for (i, item) in items.iter().enumerate() {
                if only.as_ref().is_some_and(|s| !selected(s, item)) {
                    continue;
                }
                checked += 1;
                check_ty(out, &format!("{at}[{i}]"), of, item);
            }
            if let Some(select) = only
                && checked == 0
            {
                out.push(
                    ViolationKind::NothingToCheck,
                    at.to_owned(),
                    format!(
                        "has no element with {} = {}",
                        select.field,
                        match select.equals {
                            SelectValue::Bool(b) => b.to_string(),
                            SelectValue::FalseOrMissing => "false or missing".to_owned(),
                            SelectValue::Str(s) => format!("\"{s}\""),
                        }
                    ),
                );
            }
        }
        (Ty::Object(fields), Value::Object(map)) => {
            for field in fields {
                check_field(out, at, field, map.get(field.name));
            }
        }
        _ => mismatch(out),
    }
}

fn check_field(out: &mut Out, parent: &str, field: &Field, value: Option<&Value>) {
    let at = format!("{parent}.{}", field.name);
    match (field.presence, value) {
        (Presence::Absent, Some(_)) => out.push(
            ViolationKind::Body,
            at,
            "is present but must be absent".to_owned(),
        ),
        (Presence::Absent, None) => {}
        (Presence::Always, None) => out.push(
            ViolationKind::Body,
            at,
            "is missing but always present".to_owned(),
        ),
        (Presence::Optional | Presence::Nullable, None) => {}
        (Presence::Always | Presence::Optional, Some(Value::Null)) => out.push(
            ViolationKind::Body,
            at,
            "is null but must not be".to_owned(),
        ),
        (Presence::Nullable, Some(Value::Null)) => {}
        (_, Some(value)) => check_ty(out, &at, &field.ty, value),
    }
}

#[cfg(test)]
mod tests {
    use http::{HeaderName, HeaderValue};
    use serde_json::json;

    use super::*;
    use crate::model::{Route, Variant, Verifiable};

    fn call(variant: Variant) -> Call {
        Call {
            name: "demo",
            routes: vec![Route {
                method: "GET",
                path: "/demo",
            }],
            variants: vec![variant],
        }
    }

    fn json_variant(ty: Ty) -> Call {
        call(Variant {
            name: "ok",
            status: StatusSpec::Exact(200),
            headers: vec![],
            body: BodySpec::Json(ty),
            verifiable: Verifiable::Yes,
        })
    }

    fn header_variant(format: HeaderFormat, presence: Presence) -> Call {
        call(Variant {
            name: "ok",
            status: StatusSpec::Exact(200),
            headers: vec![HeaderSpec::new("x-test", format, presence)],
            body: BodySpec::Unchecked,
            verifiable: Verifiable::Yes,
        })
    }

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

    fn body_problems(ty: Ty, body: Value) -> Vec<String> {
        check(
            &json_variant(ty),
            "ok",
            Some(200),
            &HeaderMap::new(),
            Some(&body),
        )
        .iter()
        .map(|v| format!("{} {}", v.at, v.problem))
        .collect()
    }

    fn one_field(presence: Presence) -> Ty {
        Ty::Object(vec![Field::new("f", Ty::Int, presence)])
    }

    #[test]
    fn always_requires_the_field_and_rejects_null() {
        assert!(body_problems(one_field(Presence::Always), json!({"f": 1})).is_empty());
        assert_eq!(
            body_problems(one_field(Presence::Always), json!({})),
            ["$.f is missing but always present"]
        );
        assert_eq!(
            body_problems(one_field(Presence::Always), json!({"f": null})),
            ["$.f is null but must not be"]
        );
    }

    #[test]
    fn optional_may_be_missing_but_not_null() {
        assert!(body_problems(one_field(Presence::Optional), json!({})).is_empty());
        assert!(body_problems(one_field(Presence::Optional), json!({"f": 3})).is_empty());
        assert_eq!(
            body_problems(one_field(Presence::Optional), json!({"f": null})),
            ["$.f is null but must not be"]
        );
    }

    #[test]
    fn nullable_may_be_missing_or_null() {
        assert!(body_problems(one_field(Presence::Nullable), json!({})).is_empty());
        assert!(body_problems(one_field(Presence::Nullable), json!({"f": null})).is_empty());
        assert_eq!(
            body_problems(one_field(Presence::Nullable), json!({"f": "x"})),
            ["$.f expected integer, found string"]
        );
    }

    #[test]
    fn absent_rejects_the_field_even_as_null() {
        assert!(body_problems(one_field(Presence::Absent), json!({})).is_empty());
        assert_eq!(
            body_problems(one_field(Presence::Absent), json!({"f": 1})),
            ["$.f is present but must be absent"]
        );
        assert_eq!(
            body_problems(one_field(Presence::Absent), json!({"f": null})),
            ["$.f is present but must be absent"]
        );
    }

    #[test]
    fn scalar_types_are_told_apart() {
        let ty = |t: Ty| Ty::Object(vec![Field::new("f", t, Presence::Always)]);
        assert!(body_problems(ty(Ty::Number), json!({"f": 1.5})).is_empty());
        assert!(body_problems(ty(Ty::Number), json!({"f": 2})).is_empty());
        assert_eq!(
            body_problems(ty(Ty::Int), json!({"f": 1.5})),
            ["$.f expected integer, found number"]
        );
        assert_eq!(
            body_problems(ty(Ty::Bool), json!({"f": "true"})),
            ["$.f expected boolean, found string"]
        );
        assert_eq!(
            body_problems(ty(Ty::Str), json!({"f": [1]})),
            ["$.f expected string, found array"]
        );
        assert!(body_problems(ty(Ty::DateTime), json!({"f": "2026-10-16T12:00:00Z"})).is_empty());
        assert_eq!(
            body_problems(ty(Ty::DateTime), json!({"f": "yesterday"})),
            ["$.f expected RFC 3339 date-time string, found string"]
        );
        assert!(body_problems(ty(Ty::StringOrStrings), json!({"f": "a"})).is_empty());
        assert!(body_problems(ty(Ty::StringOrStrings), json!({"f": ["a", "b"]})).is_empty());
        assert_eq!(
            body_problems(ty(Ty::StringOrStrings), json!({"f": ["a", 1]})),
            ["$.f[1] expected string, found integer"]
        );
    }

    #[test]
    fn arrays_are_checked_element_by_element_with_paths() {
        let ty = Ty::array_of(Ty::Object(vec![Field::new(
            "inner",
            Ty::array_of(Ty::Int),
            Presence::Always,
        )]));
        assert!(body_problems(ty.clone(), json!([{"inner": [1, 2]}, {"inner": []}])).is_empty());
        assert_eq!(
            body_problems(ty.clone(), json!([{"inner": [1]}, {"inner": [2, "x"]}])),
            ["$[1].inner[1] expected integer, found string"]
        );
        assert_eq!(
            body_problems(ty, json!({"inner": []})),
            ["$ expected array, found object"]
        );
    }

    #[test]
    fn a_selecting_array_checks_only_its_elements_and_needs_one() {
        let ty = Ty::array_where(
            "buy",
            SelectValue::Bool(true),
            Ty::Object(vec![Field::new("min", Ty::Int, Presence::Always)]),
        );
        assert!(
            body_problems(ty.clone(), json!([{"buy": false}, {"buy": true, "min": 1}])).is_empty()
        );
        assert_eq!(
            body_problems(ty.clone(), json!([{"buy": false}, {"buy": true}])),
            ["$[1].min is missing but always present"]
        );
        assert_eq!(
            body_problems(ty, json!([{"buy": false}])),
            ["$ has no element with buy = true"]
        );
    }

    #[test]
    fn a_missing_body_is_a_violation() {
        let violations = check(
            &json_variant(Ty::Int),
            "ok",
            Some(200),
            &HeaderMap::new(),
            None,
        );
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].at, "$");
    }

    fn header_problems(format: HeaderFormat, presence: Presence, value: Option<&str>) -> usize {
        let h = value.map_or_else(HeaderMap::new, |v| headers(&[("x-test", v)]));
        check(&header_variant(format, presence), "ok", Some(200), &h, None).len()
    }

    #[test]
    fn header_presence_kinds() {
        use HeaderFormat::Integer;
        assert_eq!(header_problems(Integer, Presence::Always, Some("5")), 0);
        assert_eq!(header_problems(Integer, Presence::Always, None), 1);
        assert_eq!(header_problems(Integer, Presence::Optional, None), 0);
        assert_eq!(header_problems(Integer, Presence::Optional, Some("x")), 1);
        assert_eq!(header_problems(Integer, Presence::Absent, None), 0);
        assert_eq!(header_problems(Integer, Presence::Absent, Some("5")), 1);
    }

    #[test]
    fn header_formats() {
        let always = Presence::Always;
        assert_eq!(
            header_problems(
                HeaderFormat::HttpDate,
                always,
                Some("Fri, 16 Oct 2026 10:00:00 GMT")
            ),
            0
        );
        assert_eq!(
            header_problems(HeaderFormat::HttpDate, always, Some("2026-10-16")),
            1
        );
        assert_eq!(
            header_problems(HeaderFormat::Integer, always, Some("-3")),
            0
        );
        assert_eq!(
            header_problems(HeaderFormat::Integer, always, Some("1.5")),
            1
        );
        assert_eq!(
            header_problems(HeaderFormat::Token, always, Some("char-market")),
            0
        );
        assert_eq!(
            header_problems(HeaderFormat::Token, always, Some("two words")),
            1
        );
        assert_eq!(
            header_problems(HeaderFormat::Text, always, Some("1200/15m")),
            0
        );
        assert_eq!(header_problems(HeaderFormat::Text, always, Some(" ")), 1);
    }

    #[test]
    fn status_is_checked_exactly_or_by_class() {
        let exact = json_variant(Ty::Int);
        assert!(check(&exact, "ok", Some(200), &HeaderMap::new(), Some(&json!(1))).is_empty());
        let wrong = check(&exact, "ok", Some(404), &HeaderMap::new(), Some(&json!(1)));
        assert_eq!(wrong[0].kind, ViolationKind::Status);
        assert_eq!(wrong[0].problem, "expected 200, got 404");

        let server = call(Variant {
            name: "ok",
            status: StatusSpec::ServerError,
            headers: vec![],
            body: BodySpec::Unchecked,
            verifiable: Verifiable::No("cannot be provoked"),
        });
        assert!(check(&server, "ok", Some(503), &HeaderMap::new(), None).is_empty());
        assert_eq!(
            check(&server, "ok", Some(404), &HeaderMap::new(), None).len(),
            1
        );
    }

    #[test]
    fn an_unknown_variant_is_a_violation() {
        let v = check(
            &json_variant(Ty::Int),
            "nope",
            None,
            &HeaderMap::new(),
            None,
        );
        assert_eq!(v.len(), 1);
    }

    #[test]
    fn violations_name_call_variant_and_location_but_never_values() {
        let ty = Ty::Object(vec![
            Field::new("secret", Ty::Int, Presence::Always),
            Field::new("token", Ty::Str, Presence::Always),
        ]);
        let violations = check(
            &json_variant(ty),
            "ok",
            Some(200),
            &headers(&[("x-test", "header-secret-value")]),
            Some(&json!({"secret": "hunter2", "token": 1234567})),
        );
        let text: Vec<String> = violations.iter().map(ToString::to_string).collect();
        assert_eq!(
            text,
            [
                "demo/ok: $.secret: expected integer, found string",
                "demo/ok: $.token: expected string, found integer",
            ]
        );
        let all = text.join("\n");
        assert!(!all.contains("hunter2") && !all.contains("1234567"));

        let header = check(
            &header_variant(HeaderFormat::Integer, Presence::Always),
            "ok",
            Some(200),
            &headers(&[("x-test", "header-secret-value")]),
            None,
        );
        assert!(!header[0].to_string().contains("header-secret-value"));
        assert_eq!(
            header[0].to_string(),
            "demo/ok: header x-test: does not have the format Integer"
        );
    }
}
