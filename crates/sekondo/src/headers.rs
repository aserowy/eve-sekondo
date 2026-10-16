//! Security headers added to every response.

use axum::{
    Router,
    http::{HeaderName, HeaderValue, header},
};
use tower_http::set_header::SetResponseHeaderLayer;
use url::Url;

fn value(s: &str) -> HeaderValue {
    HeaderValue::from_str(s).expect("static header value")
}

pub fn content_security_policy(sso_base_url: &Url) -> String {
    let sso_origin = sso_base_url.origin().ascii_serialization();
    format!(
        "default-src 'self'; img-src 'self' https://images.evetech.net; script-src 'self'; \
         style-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'self' {sso_origin}"
    )
}

pub fn apply<S: Clone + Send + Sync + 'static>(router: Router<S>, sso_base_url: &Url) -> Router<S> {
    let layer = |name: HeaderName, v: &str| SetResponseHeaderLayer::overriding(name, value(v));
    router
        .layer(layer(
            header::CONTENT_SECURITY_POLICY,
            &content_security_policy(sso_base_url),
        ))
        .layer(layer(header::STRICT_TRANSPORT_SECURITY, "max-age=31536000"))
        .layer(layer(header::X_CONTENT_TYPE_OPTIONS, "nosniff"))
        .layer(layer(header::REFERRER_POLICY, "same-origin"))
        .layer(layer(header::X_FRAME_OPTIONS, "DENY"))
}
