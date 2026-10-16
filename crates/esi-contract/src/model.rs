//! What a contract is made of. All of it is plain data, so it can be serialized into the stub
//! fingerprint.

use serde::Serialize;

/// One call the app makes to ESI or EVE SSO, with one variant per answer it handles differently.
#[derive(Debug, Clone, Serialize)]
pub struct Call {
    pub name: &'static str,
    /// The stub routes that serve this call (method and axum path pattern).
    pub routes: Vec<Route>,
    pub variants: Vec<Variant>,
}

impl Call {
    pub fn variant(&self, name: &str) -> Option<&Variant> {
        self.variants.iter().find(|v| v.name == name)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Route {
    pub method: &'static str,
    pub path: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct Variant {
    pub name: &'static str,
    pub status: StatusSpec,
    pub headers: Vec<HeaderSpec>,
    pub body: BodySpec,
    pub verifiable: Verifiable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum StatusSpec {
    /// Not an HTTP answer: a request the app builds, or the claims inside a token.
    NotHttp,
    Exact(u16),
    /// Any 5xx.
    ServerError,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Verifiable {
    Yes,
    /// Cannot be provoked against the real services without harm.
    No(&'static str),
}

#[derive(Debug, Clone, Serialize)]
pub struct HeaderSpec {
    pub name: &'static str,
    pub format: HeaderFormat,
    /// `Always`, `Optional` or `Absent`.
    pub presence: Presence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum HeaderFormat {
    /// `Fri, 16 Oct 2026 10:00:00 GMT`
    HttpDate,
    Integer,
    /// A single visible-ASCII word without whitespace.
    Token,
    /// Any non-empty visible text.
    Text,
}

#[derive(Debug, Clone, Serialize)]
pub enum BodySpec {
    /// The body is not looked at.
    Unchecked,
    Json(Ty),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Presence {
    /// Present and not null.
    Always,
    /// May be missing; when present, not null.
    Optional,
    /// May be missing or null.
    Nullable,
    /// Must not be there at all (not even as null).
    Absent,
}

#[derive(Debug, Clone, Serialize)]
pub struct Field {
    pub name: &'static str,
    pub ty: Ty,
    pub presence: Presence,
}

#[derive(Debug, Clone, Serialize)]
pub enum Ty {
    Bool,
    Int,
    /// Any JSON number.
    Number,
    Str,
    /// A string in RFC 3339 form.
    DateTime,
    /// A string, or an array of strings (EVE sends one scope as a string and several as an array).
    StringOrStrings,
    Array {
        of: Box<Ty>,
        /// Only the elements with this field value are checked (and at least one must exist).
        only: Option<Select>,
    },
    Object(Vec<Field>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Select {
    pub field: &'static str,
    pub equals: SelectValue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum SelectValue {
    Bool(bool),
    /// The field is missing or `false` (ESI leaves `is_buy_order` out of sell orders).
    FalseOrMissing,
    Str(&'static str),
}

impl Ty {
    pub fn array_of(of: Ty) -> Self {
        Self::Array {
            of: Box::new(of),
            only: None,
        }
    }

    pub fn array_where(field: &'static str, equals: SelectValue, of: Ty) -> Self {
        Self::Array {
            of: Box::new(of),
            only: Some(Select { field, equals }),
        }
    }
}

impl Field {
    pub fn new(name: &'static str, ty: Ty, presence: Presence) -> Self {
        Self { name, ty, presence }
    }
}

impl HeaderSpec {
    pub fn new(name: &'static str, format: HeaderFormat, presence: Presence) -> Self {
        Self {
            name,
            format,
            presence,
        }
    }
}
