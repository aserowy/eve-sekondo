use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde_json::Value;

/// The claims of a JWT, without verifying anything: for checking their shape against the contract.
pub fn jwt_payload(token: &str) -> Option<Value> {
    let payload = token.split('.').nth(1)?;
    serde_json::from_slice(&URL_SAFE_NO_PAD.decode(payload).ok()?).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_payload_segment() {
        let payload = URL_SAFE_NO_PAD.encode(br#"{"sub":"x"}"#);
        let token = format!("e30.{payload}.sig");
        assert_eq!(jwt_payload(&token), Some(serde_json::json!({"sub": "x"})));
        assert_eq!(jwt_payload("nonsense"), None);
    }
}
