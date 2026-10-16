//! Error texts without the data that caused them.
//!
//! serde's type errors quote the offending value (`invalid type: string "abc", expected i64`),
//! which for real answers could be an order or a token. Only the kind of value is kept.

/// `invalid type: string "x", expected i64` becomes `invalid type: string, expected i64`.
pub fn scrub_decode_error(message: &str) -> String {
    for prefix in ["invalid type: ", "invalid value: "] {
        if let Some(rest) = message.strip_prefix(prefix) {
            let kind = if rest.starts_with("floating point") {
                "floating point"
            } else {
                rest.split([' ', ',', '`', '"']).next().unwrap_or_default()
            };
            let tail = rest
                .find(", expected")
                .map_or(String::new(), |i| rest[i..].to_owned());
            return format!("{prefix}{kind}{tail}");
        }
    }
    // `unknown variant`, `unknown field` and the like quote the value in backticks.
    let mut out = String::with_capacity(message.len());
    let mut in_ticks = false;
    for c in message.chars() {
        match (c, in_ticks) {
            ('`', false) if message.starts_with("missing field") => {
                out.push(c);
                in_ticks = false;
            }
            ('`', false) => {
                in_ticks = true;
                out.push_str("`…`");
            }
            ('`', true) => in_ticks = false,
            (_, true) => {}
            (c, false) => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type_errors_keep_the_kind_and_drop_the_value() {
        assert_eq!(
            scrub_decode_error(
                r#"invalid type: string "hunter2", expected f64 at line 1 column 20"#
            ),
            "invalid type: string, expected f64 at line 1 column 20"
        );
        assert_eq!(
            scrub_decode_error(
                "invalid type: floating point `1.5`, expected i64 at line 1 column 3"
            ),
            "invalid type: floating point, expected i64 at line 1 column 3"
        );
        assert_eq!(
            scrub_decode_error("invalid type: null, expected a string at line 1 column 9"),
            "invalid type: null, expected a string at line 1 column 9"
        );
    }

    #[test]
    fn missing_fields_keep_their_name() {
        assert_eq!(
            scrub_decode_error("missing field `type_id` at line 1 column 40"),
            "missing field `type_id` at line 1 column 40"
        );
    }

    #[test]
    fn other_quoted_values_are_dropped() {
        assert_eq!(
            scrub_decode_error("unknown variant `secret`, expected one of `a`, `b`"),
            "unknown variant `…`, expected one of `…`, `…`"
        );
    }
}
