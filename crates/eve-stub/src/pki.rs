//! Throwaway certificates for tests and local runs: a CA plus a localhost server certificate.

use std::path::{Path, PathBuf};

use rcgen::{BasicConstraints, CertificateParams, IsCa, Issuer, KeyPair, KeyUsagePurpose, SanType};

pub struct TestPki {
    pub ca_pem: String,
    pub cert_pem: String,
    pub key_pem: String,
}

impl TestPki {
    pub fn generate() -> Self {
        let ca_key = KeyPair::generate().expect("ca key");
        let mut ca_params = CertificateParams::new(Vec::<String>::new()).expect("ca params");
        ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        ca_params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
        let ca = ca_params.self_signed(&ca_key).expect("ca cert");

        let key = KeyPair::generate().expect("server key");
        let mut params = CertificateParams::new(vec!["localhost".to_owned()]).expect("params");
        params
            .subject_alt_names
            .push(SanType::IpAddress("127.0.0.1".parse().expect("ip")));
        let cert = params
            .signed_by(&key, &Issuer::from_params(&ca_params, &ca_key))
            .expect("server cert");

        Self {
            ca_pem: ca.pem(),
            cert_pem: cert.pem(),
            key_pem: key.serialize_pem(),
        }
    }

    /// Writes `ca.pem`, `cert.pem` and `key.pem` into `dir` and returns their paths (ca, cert, key).
    pub fn write(&self, dir: &Path) -> std::io::Result<(PathBuf, PathBuf, PathBuf)> {
        let write = |name: &str, content: &str| -> std::io::Result<PathBuf> {
            let path = dir.join(name);
            std::fs::write(&path, content)?;
            Ok(path)
        };
        Ok((
            write("ca.pem", &self.ca_pem)?,
            write("cert.pem", &self.cert_pem)?,
            write("key.pem", &self.key_pem)?,
        ))
    }

    /// An HTTP client that trusts this CA (plus the system roots) and never follows redirects.
    pub fn client(&self) -> reqwest::Client {
        reqwest::Client::builder()
            .add_root_certificate(
                reqwest::Certificate::from_pem(self.ca_pem.as_bytes()).expect("ca"),
            )
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("client")
    }
}
