//! Typed configuration, parsed once at startup. There is deliberately no switch that
//! relaxes a security control: HTTPS everywhere, verify-full database TLS.

use std::{fs::File, net::SocketAddr, path::PathBuf};

use base64::{Engine, engine::general_purpose::STANDARD};
use secrecy::SecretString;
use url::Url;

const DEFAULT_BIND_ADDR: &str = "0.0.0.0:8443";
const DEFAULT_SSO_BASE_URL: &str = "https://login.eveonline.com";
const DEFAULT_ESI_BASE_URL: &str = "https://esi.evetech.net";
const DEFAULT_POLL_INTERVAL_MINUTES: &str = "20";
const POLL_INTERVAL_RANGE: std::ops::RangeInclusive<i64> = 1..=1440;

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("{var} is required but not set")]
    Missing { var: &'static str },
    #[error("{var} is invalid: {reason}")]
    Invalid { var: &'static str, reason: String },
}

fn invalid(var: &'static str, reason: impl Into<String>) -> ConfigError {
    ConfigError::Invalid {
        var,
        reason: reason.into(),
    }
}

#[derive(Debug)]
pub struct Config {
    pub database_url: SecretString,
    pub bind_addr: SocketAddr,
    pub tls_cert_path: PathBuf,
    pub tls_key_path: PathBuf,
    pub tls_extra_ca_path: Option<PathBuf>,
    pub public_base_url: Url,
    pub eve_client_id: String,
    pub eve_client_secret: SecretString,
    pub eve_sso_base_url: Url,
    pub esi_base_url: Url,
    pub token_encryption_key: SecretKey,
    pub static_dir: PathBuf,
    /// How often each character's orders are polled (1 to 1440 minutes).
    pub order_poll_interval: time::Duration,
    /// Whether the background sync of the trading station's market runs. Browser tests turn it
    /// off and drive it through the test hooks instead.
    pub station_sync: bool,
    /// Appended to the ESI `User-Agent` so CCP can reach the operator.
    pub esi_contact: Option<String>,
}

/// 32-byte AES-256 key. Never printed.
#[derive(Clone)]
pub struct SecretKey(pub [u8; 32]);

impl std::fmt::Debug for SecretKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SecretKey(<redacted>)")
    }
}

impl Config {
    /// Loads `.env` (if present) and reads the process environment.
    pub fn from_env() -> Result<Self, ConfigError> {
        let _ = dotenvy::dotenv();
        Self::from_lookup(|name| std::env::var(name).ok())
    }

    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let required = |var: &'static str| {
            get(var)
                .filter(|v| !v.is_empty())
                .ok_or(ConfigError::Missing { var })
        };
        let or_default = |var: &'static str, default: &str| {
            get(var)
                .filter(|v| !v.is_empty())
                .unwrap_or_else(|| default.to_owned())
        };

        let database_url = required("DATABASE_URL")?;
        check_database_url(&database_url)?;

        let bind_addr = or_default("BIND_ADDR", DEFAULT_BIND_ADDR)
            .parse()
            .map_err(|e| invalid("BIND_ADDR", format!("{e}")))?;

        let tls_cert_path = readable_file("TLS_CERT_PATH", required("TLS_CERT_PATH")?)?;
        let tls_key_path = readable_file("TLS_KEY_PATH", required("TLS_KEY_PATH")?)?;
        let tls_extra_ca_path = match get("TLS_EXTRA_CA_PATH").filter(|v| !v.is_empty()) {
            Some(p) => Some(readable_file("TLS_EXTRA_CA_PATH", p)?),
            None => None,
        };

        let public_base_url = https_url("PUBLIC_BASE_URL", &required("PUBLIC_BASE_URL")?)?;
        let eve_sso_base_url = https_url(
            "EVE_SSO_BASE_URL",
            &or_default("EVE_SSO_BASE_URL", DEFAULT_SSO_BASE_URL),
        )?;
        let esi_base_url = https_url(
            "ESI_BASE_URL",
            &or_default("ESI_BASE_URL", DEFAULT_ESI_BASE_URL),
        )?;

        let order_poll_interval = parse_poll_interval(&or_default(
            "ORDER_POLL_INTERVAL_MINUTES",
            DEFAULT_POLL_INTERVAL_MINUTES,
        ))?;

        let station_sync = parse_switch("STATION_SYNC", &or_default("STATION_SYNC", "on"))?;

        Ok(Self {
            database_url: SecretString::from(database_url),
            bind_addr,
            tls_cert_path,
            tls_key_path,
            tls_extra_ca_path,
            public_base_url,
            eve_client_id: required("EVE_CLIENT_ID")?,
            eve_client_secret: SecretString::from(required("EVE_CLIENT_SECRET")?),
            eve_sso_base_url,
            esi_base_url,
            token_encryption_key: parse_key(&required("TOKEN_ENCRYPTION_KEY")?)?,
            static_dir: get("STATIC_DIR")
                .filter(|v| !v.is_empty())
                .map_or_else(default_static_dir, PathBuf::from),
            order_poll_interval,
            station_sync,
            esi_contact: get("ESI_CONTACT")
                .map(|v| v.trim().to_owned())
                .filter(|v| !v.is_empty()),
        })
    }
}

/// The vendored assets live next to the crate sources; deployments override with `STATIC_DIR`.
fn default_static_dir() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/static"))
}

fn parse_switch(var: &'static str, value: &str) -> Result<bool, ConfigError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "on" | "true" | "1" | "yes" => Ok(true),
        "off" | "false" | "0" | "no" => Ok(false),
        other => Err(invalid(var, format!("{other:?} is neither on nor off"))),
    }
}

fn parse_poll_interval(value: &str) -> Result<time::Duration, ConfigError> {
    const VAR: &str = "ORDER_POLL_INTERVAL_MINUTES";
    let minutes: i64 = value
        .trim()
        .parse()
        .map_err(|_| invalid(VAR, "must be a whole number of minutes"))?;
    if !POLL_INTERVAL_RANGE.contains(&minutes) {
        return Err(invalid(VAR, "must be between 1 and 1440 minutes"));
    }
    Ok(time::Duration::minutes(minutes))
}

fn https_url(var: &'static str, value: &str) -> Result<Url, ConfigError> {
    let url = Url::parse(value).map_err(|e| invalid(var, format!("not a valid URL: {e}")))?;
    if url.scheme() != "https" {
        return Err(invalid(var, "must use https://"));
    }
    Ok(url)
}

fn readable_file(var: &'static str, path: String) -> Result<PathBuf, ConfigError> {
    let path = PathBuf::from(path);
    File::open(&path).map_err(|e| invalid(var, format!("cannot read {}: {e}", path.display())))?;
    Ok(path)
}

fn parse_key(value: &str) -> Result<SecretKey, ConfigError> {
    let bytes = STANDARD
        .decode(value.trim())
        .map_err(|_| invalid("TOKEN_ENCRYPTION_KEY", "not valid base64"))?;
    let key: [u8; 32] = bytes.try_into().map_err(|b: Vec<u8>| {
        invalid(
            "TOKEN_ENCRYPTION_KEY",
            format!("must decode to 32 bytes, got {}", b.len()),
        )
    })?;
    Ok(SecretKey(key))
}

fn check_database_url(value: &str) -> Result<(), ConfigError> {
    let url = Url::parse(value).map_err(|_| invalid("DATABASE_URL", "not a valid URL"))?;
    if !matches!(url.scheme(), "postgres" | "postgresql") {
        return Err(invalid("DATABASE_URL", "must be a postgres:// URL"));
    }
    let param = |name: &str| {
        url.query_pairs()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.into_owned())
    };
    if param("sslmode").as_deref() != Some("verify-full") {
        return Err(invalid("DATABASE_URL", "must set sslmode=verify-full"));
    }
    if param("sslrootcert").is_none_or(|v| v.is_empty()) {
        return Err(invalid("DATABASE_URL", "must set sslrootcert=<CA file>"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, io::Write};

    use secrecy::ExposeSecret;
    use tempfile::NamedTempFile;

    use super::*;

    struct Fixture {
        vars: HashMap<&'static str, String>,
        _files: Vec<NamedTempFile>,
    }

    impl Fixture {
        fn valid() -> Self {
            let mut files = Vec::new();
            let mut file = || {
                let mut f = NamedTempFile::new().unwrap();
                f.write_all(b"pem").unwrap();
                let path = f.path().display().to_string();
                files.push(f);
                path
            };
            let (cert, key) = (file(), file());
            let vars = HashMap::from([
                (
                    "DATABASE_URL",
                    "postgres://u:p@localhost:5432/sekondo?sslmode=verify-full&sslrootcert=/ca.pem"
                        .to_owned(),
                ),
                ("TLS_CERT_PATH", cert),
                ("TLS_KEY_PATH", key),
                ("PUBLIC_BASE_URL", "https://localhost:8443".to_owned()),
                ("EVE_CLIENT_ID", "id".to_owned()),
                ("EVE_CLIENT_SECRET", "secret".to_owned()),
                ("TOKEN_ENCRYPTION_KEY", STANDARD.encode([7u8; 32])),
            ]);
            Self {
                vars,
                _files: files,
            }
        }

        fn set(mut self, k: &'static str, v: &str) -> Self {
            self.vars.insert(k, v.to_owned());
            self
        }

        fn without(mut self, k: &'static str) -> Self {
            self.vars.remove(k);
            self
        }

        fn load(&self) -> Result<Config, ConfigError> {
            Config::from_lookup(|k| self.vars.get(k).cloned())
        }

        fn error(&self) -> String {
            self.load().unwrap_err().to_string()
        }
    }

    #[test]
    fn valid_all_https_verify_full_config_is_accepted() {
        let config = Fixture::valid().load().unwrap();
        assert_eq!(config.token_encryption_key.0, [7u8; 32]);
        assert_eq!(config.eve_client_secret.expose_secret(), "secret");
    }

    #[test]
    fn missing_setting_names_the_variable() {
        let err = Fixture::valid().without("EVE_CLIENT_ID").error();
        assert!(err.contains("EVE_CLIENT_ID"), "{err}");
    }

    #[test]
    fn invalid_encryption_key_length_is_rejected() {
        let err = Fixture::valid()
            .set("TOKEN_ENCRYPTION_KEY", &STANDARD.encode([1u8; 16]))
            .error();
        assert!(
            err.contains("TOKEN_ENCRYPTION_KEY") && err.contains("32"),
            "{err}"
        );
    }

    #[test]
    fn urls_and_bind_addr_default() {
        let config = Fixture::valid().load().unwrap();
        assert_eq!(
            config.eve_sso_base_url.as_str(),
            "https://login.eveonline.com/"
        );
        assert_eq!(config.esi_base_url.as_str(), "https://esi.evetech.net/");
        assert_eq!(config.bind_addr.to_string(), "0.0.0.0:8443");
    }

    #[test]
    fn http_urls_are_rejected_without_localhost_exception() {
        for (var, value) in [
            ("EVE_SSO_BASE_URL", "http://localhost:9000"),
            ("EVE_SSO_BASE_URL", "http://login.example.com"),
            ("ESI_BASE_URL", "http://localhost:9000"),
            ("ESI_BASE_URL", "http://esi.example.com"),
            ("PUBLIC_BASE_URL", "http://localhost:8443"),
        ] {
            let err = Fixture::valid().set(var, value).error();
            assert!(
                err.contains(var) && err.contains("https"),
                "{var}={value}: {err}"
            );
        }
    }

    #[test]
    fn poll_interval_defaults_to_twenty_minutes() {
        let config = Fixture::valid().load().unwrap();
        assert_eq!(config.order_poll_interval, time::Duration::minutes(20));
        assert_eq!(config.esi_contact, None);
    }

    #[test]
    fn poll_interval_and_contact_are_read() {
        let config = Fixture::valid()
            .set("ORDER_POLL_INTERVAL_MINUTES", "1440")
            .set("ESI_CONTACT", "trader@example.com")
            .load()
            .unwrap();
        assert_eq!(config.order_poll_interval, time::Duration::minutes(1440));
        assert_eq!(config.esi_contact.as_deref(), Some("trader@example.com"));
    }

    #[test]
    fn invalid_poll_interval_names_the_variable() {
        for value in ["0", "1441", "abc", "-5", "2.5"] {
            let err = Fixture::valid()
                .set("ORDER_POLL_INTERVAL_MINUTES", value)
                .error();
            assert!(
                err.contains("ORDER_POLL_INTERVAL_MINUTES"),
                "{value}: {err}"
            );
        }
    }

    #[test]
    fn station_sync_is_on_by_default_and_can_be_switched_off() {
        assert!(Fixture::valid().load().unwrap().station_sync);
        for value in ["on", "true", "1", "yes", "ON"] {
            let config = Fixture::valid().set("STATION_SYNC", value).load().unwrap();
            assert!(config.station_sync, "{value}");
        }
        for value in ["off", "false", "0", "no", "Off"] {
            let config = Fixture::valid().set("STATION_SYNC", value).load().unwrap();
            assert!(!config.station_sync, "{value}");
        }
    }

    #[test]
    fn an_unknown_station_sync_value_names_the_variable() {
        let err = Fixture::valid().set("STATION_SYNC", "maybe").error();
        assert!(err.contains("STATION_SYNC"), "{err}");
    }

    #[test]
    fn missing_certificate_is_rejected() {
        let err = Fixture::valid()
            .set("TLS_CERT_PATH", "/nonexistent/dev.pem")
            .error();
        assert!(err.contains("TLS_CERT_PATH"), "{err}");
        let err = Fixture::valid().without("TLS_KEY_PATH").error();
        assert!(err.contains("TLS_KEY_PATH"), "{err}");
    }

    #[test]
    fn weak_database_tls_is_rejected() {
        for url in [
            "postgres://u:p@h/db?sslmode=require&sslrootcert=/ca.pem",
            "postgres://u:p@h/db?sslmode=verify-full",
            "postgres://u:p@h/db?sslrootcert=/ca.pem",
            "postgres://u:p@h/db",
        ] {
            let err = Fixture::valid().set("DATABASE_URL", url).error();
            assert!(err.contains("DATABASE_URL"), "{url}: {err}");
        }
    }
}

#[cfg(test)]
mod env_example_tests {
    use std::collections::HashMap;

    use super::*;

    /// Keeps `.env.example` in step with the fields `Config` reads.
    #[test]
    fn env_example_documents_every_config_setting_and_loads() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../.env.example");
        let mut vars: HashMap<String, String> = dotenvy::from_path_iter(path)
            .unwrap()
            .map(Result::unwrap)
            .collect();

        for name in [
            "DATABASE_URL",
            "BIND_ADDR",
            "TLS_CERT_PATH",
            "TLS_KEY_PATH",
            "TLS_EXTRA_CA_PATH",
            "PUBLIC_BASE_URL",
            "EVE_CLIENT_ID",
            "EVE_CLIENT_SECRET",
            "EVE_SSO_BASE_URL",
            "ESI_BASE_URL",
            "TOKEN_ENCRYPTION_KEY",
            "STATIC_DIR",
            "ORDER_POLL_INTERVAL_MINUTES",
            "STATION_SYNC",
            "ESI_CONTACT",
        ] {
            assert!(vars.contains_key(name), "{name} missing from .env.example");
        }

        // Fill the secrets and point the certificate paths at real files.
        let cargo_toml = concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml").to_owned();
        for k in ["TLS_CERT_PATH", "TLS_KEY_PATH", "TLS_EXTRA_CA_PATH"] {
            vars.insert(k.into(), cargo_toml.clone());
        }
        vars.insert("EVE_CLIENT_ID".into(), "id".into());
        vars.insert("EVE_CLIENT_SECRET".into(), "secret".into());
        vars.insert("TOKEN_ENCRYPTION_KEY".into(), STANDARD.encode([0u8; 32]));

        Config::from_lookup(|k| vars.get(k).cloned()).unwrap();
    }
}
