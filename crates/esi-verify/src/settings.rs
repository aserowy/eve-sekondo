use std::{net::SocketAddr, path::PathBuf, time::Duration};

use secrecy::SecretString;
use url::Url;

pub const REAL_SSO: &str = "https://login.eveonline.com";
pub const REAL_ESI: &str = "https://esi.evetech.net";

pub struct Settings {
    pub sso_base_url: Url,
    pub esi_base_url: Url,
    pub client_id: String,
    pub client_secret: SecretString,
    /// The callback is `{public_base_url}/auth/callback`, listened on at its port on loopback.
    pub public_base_url: Url,
    pub tls_cert_path: PathBuf,
    pub tls_key_path: PathBuf,
    /// Extra trust for outbound HTTPS (tests talk to the stub).
    pub extra_ca_path: Option<PathBuf>,
    pub contact: Option<String>,
    /// A structure the verification character can open, and one it cannot.
    pub structure_id: i64,
    pub forbidden_structure_id: i64,
    pub workspace_root: PathBuf,
    pub login_timeout: Duration,
}

impl Settings {
    /// Where the login ends: loopback at the port of `PUBLIC_BASE_URL`.
    pub fn callback_addr(&self) -> Result<SocketAddr, String> {
        let port = self
            .public_base_url
            .port_or_known_default()
            .ok_or("PUBLIC_BASE_URL has no port")?;
        Ok(SocketAddr::from(([127, 0, 0, 1], port)))
    }

    pub fn callback_url(&self) -> String {
        format!(
            "{}/auth/callback",
            self.public_base_url.as_str().trim_end_matches('/')
        )
    }

    /// Reads the verifier's settings from the environment (`.env` is loaded by the caller).
    pub fn from_lookup(
        get: impl Fn(&str) -> Option<String>,
        workspace_root: PathBuf,
    ) -> Result<Self, String> {
        let required = |var: &str| {
            get(var)
                .filter(|v| !v.is_empty())
                .ok_or_else(|| format!("{var} is required but not set (see .env.example)"))
        };
        let url = |var: &str, default: Option<&str>| -> Result<Url, String> {
            let value = match default {
                Some(default) => get(var)
                    .filter(|v| !v.is_empty())
                    .unwrap_or_else(|| default.to_owned()),
                None => required(var)?,
            };
            let url = Url::parse(&value).map_err(|e| format!("{var} is not a URL: {e}"))?;
            if url.scheme() != "https" {
                return Err(format!("{var} must be an https:// URL"));
            }
            Ok(url)
        };
        let id = |var: &str| -> Result<i64, String> {
            required(var)?
                .trim()
                .parse()
                .map_err(|_| format!("{var} must be a structure ID (a number)"))
        };
        let settings = Self {
            sso_base_url: url("EVE_SSO_BASE_URL", Some(REAL_SSO))?,
            esi_base_url: url("ESI_BASE_URL", Some(REAL_ESI))?,
            client_id: required("EVE_CLIENT_ID")?,
            client_secret: SecretString::from(required("EVE_CLIENT_SECRET")?),
            public_base_url: url("PUBLIC_BASE_URL", None)?,
            tls_cert_path: PathBuf::from(required("TLS_CERT_PATH")?),
            tls_key_path: PathBuf::from(required("TLS_KEY_PATH")?),
            extra_ca_path: None,
            contact: get("ESI_CONTACT")
                .map(|v| v.trim().to_owned())
                .filter(|v| !v.is_empty()),
            structure_id: id("ESI_VERIFY_STRUCTURE_ID")?,
            forbidden_structure_id: id("ESI_VERIFY_FORBIDDEN_STRUCTURE_ID")?,
            workspace_root,
            login_timeout: Duration::from_secs(300),
        };
        Ok(settings)
    }

    /// The command verifies against real EVE, never against the stub: otherwise a run against the
    /// stub would record the stub as verified.
    pub fn require_real_eve(&self) -> Result<(), String> {
        for (var, url, real) in [
            ("EVE_SSO_BASE_URL", &self.sso_base_url, REAL_SSO),
            ("ESI_BASE_URL", &self.esi_base_url, REAL_ESI),
        ] {
            if url.as_str().trim_end_matches('/') != real {
                return Err(format!(
                    "{var} must be {real}: esi-verify checks real EVE, not a stand-in"
                ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lookup(extra: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let mut vars: Vec<(String, String)> = [
            ("EVE_CLIENT_ID", "id"),
            ("EVE_CLIENT_SECRET", "secret"),
            ("PUBLIC_BASE_URL", "https://localhost:8443"),
            ("TLS_CERT_PATH", "c.pem"),
            ("TLS_KEY_PATH", "k.pem"),
            ("ESI_VERIFY_STRUCTURE_ID", "1000000000001"),
            ("ESI_VERIFY_FORBIDDEN_STRUCTURE_ID", "1000000000002"),
        ]
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .into();
        vars.extend(
            extra
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned())),
        );
        move |name| {
            vars.iter()
                .rev()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.clone())
        }
    }

    fn settings(extra: &[(&str, &str)]) -> Result<Settings, String> {
        Settings::from_lookup(lookup(extra), PathBuf::from("."))
    }

    #[test]
    fn defaults_are_the_real_services() {
        let s = settings(&[]).unwrap();
        assert!(s.require_real_eve().is_ok());
        assert_eq!(s.callback_url(), "https://localhost:8443/auth/callback");
        assert_eq!(s.callback_addr().unwrap().to_string(), "127.0.0.1:8443");
    }

    #[test]
    fn a_stand_in_is_refused() {
        let s = settings(&[("EVE_SSO_BASE_URL", "https://localhost:9443")]).unwrap();
        let message = s.require_real_eve().unwrap_err();
        assert!(message.contains("EVE_SSO_BASE_URL"), "{message}");
        let s = settings(&[("ESI_BASE_URL", "https://localhost:9443")]).unwrap();
        assert!(s.require_real_eve().unwrap_err().contains("ESI_BASE_URL"));
    }

    #[test]
    fn the_structure_ids_are_required_and_named() {
        let get =
            |name: &str| lookup(&[])(name).filter(|_| name != "ESI_VERIFY_FORBIDDEN_STRUCTURE_ID");
        let message = Settings::from_lookup(get, PathBuf::from("."))
            .err()
            .unwrap();
        assert!(
            message.contains("ESI_VERIFY_FORBIDDEN_STRUCTURE_ID"),
            "{message}"
        );
        let message = settings(&[("ESI_VERIFY_STRUCTURE_ID", "abc")])
            .err()
            .unwrap();
        assert!(message.contains("ESI_VERIFY_STRUCTURE_ID"), "{message}");
    }

    #[test]
    fn http_is_refused() {
        assert!(settings(&[("PUBLIC_BASE_URL", "http://localhost:8443")]).is_err());
    }
}
