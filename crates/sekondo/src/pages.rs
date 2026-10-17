use askama::Template;
use axum::{
    extract::State,
    http::StatusCode,
    response::{Html, IntoResponse, Redirect, Response},
};
use tower_sessions::Session;

use crate::{account::CurrentAccount, app::AppState, auth, csrf, selection::ViewQuery};

/// The signed-in pages the navigation links to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Dashboard,
    Orders,
    Wealth,
    Opportunities,
    Characters,
}

/// The signed-in navigation: which page is current, and the character filter its links carry.
#[derive(Debug, Clone)]
pub struct Nav {
    current: Page,
    /// `""`, or `?chars=<ids>`: appended to links to views that accept the filter.
    chars_query: String,
}

impl Nav {
    pub fn new(current: Page) -> Self {
        Self {
            current,
            chars_query: String::new(),
        }
    }

    pub fn with_chars_query(mut self, chars_query: String) -> Self {
        self.chars_query = chars_query;
        self
    }

    pub fn dashboard_current(&self) -> bool {
        self.current == Page::Dashboard
    }

    pub fn orders_current(&self) -> bool {
        self.current == Page::Orders
    }

    pub fn wealth_current(&self) -> bool {
        self.current == Page::Wealth
    }

    pub fn opportunities_current(&self) -> bool {
        self.current == Page::Opportunities
    }

    pub fn characters_current(&self) -> bool {
        self.current == Page::Characters
    }

    pub fn orders_href(&self) -> String {
        format!("/orders{}", self.chars_query)
    }

    /// Carries the selection and never the range: the wealth page has a range of its own.
    pub fn wealth_href(&self) -> String {
        format!("/wealth{}", self.chars_query)
    }

    /// Carries the selection and never the page's own sorting or filters.
    pub fn opportunities_href(&self) -> String {
        format!("/opportunities{}", self.chars_query)
    }

    pub fn dashboard_href(&self) -> String {
        format!("/dashboard{}", self.chars_query)
    }

    pub fn characters_href(&self) -> String {
        format!("/characters{}", self.chars_query)
    }
}

#[derive(Template)]
#[template(path = "index.html")]
pub struct IndexTemplate {
    pub csrf_token: String,
    pub nav: Option<Nav>,
}

#[derive(Template)]
#[template(path = "error.html")]
pub struct ErrorTemplate {
    pub csrf_token: String,
    pub nav: Option<Nav>,
    pub title: String,
    pub message: String,
}

pub fn render<T: Template>(template: &T) -> Response {
    render_with(StatusCode::OK, template)
}

pub fn render_with<T: Template>(status: StatusCode, template: &T) -> Response {
    match template.render() {
        Ok(body) => (status, Html(body)).into_response(),
        Err(e) => {
            tracing::error!("template rendering failed: {e}");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

pub fn internal_error(e: impl std::fmt::Display) -> Response {
    tracing::error!("internal error: {e}");
    StatusCode::INTERNAL_SERVER_ERROR.into_response()
}

/// An error page that offers to try again.
pub async fn error_page(
    session: &Session,
    status: StatusCode,
    title: &str,
    message: &str,
) -> Response {
    match csrf::token(session).await {
        Ok(csrf_token) => render_with(
            status,
            &ErrorTemplate {
                csrf_token,
                nav: None,
                title: title.to_owned(),
                message: message.to_owned(),
            },
        ),
        Err(e) => internal_error(e),
    }
}

pub struct CharacterView {
    pub id: i64,
    pub name: String,
    pub needs_relogin: bool,
}

#[derive(Template)]
#[template(path = "characters.html")]
pub struct CharactersTemplate {
    pub csrf_token: String,
    pub nav: Option<Nav>,
    pub characters: Vec<CharacterView>,
    pub flash: Option<String>,
}

/// The landing page. Signed-in users go straight to the dashboard.
pub async fn index(account: Option<CurrentAccount>, session: Session) -> Response {
    if account.is_some() {
        return Redirect::to("/dashboard").into_response();
    }
    match csrf::token(&session).await {
        Ok(csrf_token) => render(&IndexTemplate {
            csrf_token,
            nav: None,
        }),
        Err(e) => internal_error(e),
    }
}

pub async fn characters(
    State(state): State<AppState>,
    account: CurrentAccount,
    session: Session,
    query: ViewQuery,
) -> Response {
    let records = match state.repo.list_characters(account.id).await {
        Ok(records) => records,
        Err(e) => return internal_error(e),
    };
    if records.is_empty() {
        // An account without characters cannot exist; the session outlived its account.
        let _ = session.flush().await;
        return Redirect::to("/").into_response();
    }
    let csrf_token = match csrf::token(&session).await {
        Ok(token) => token,
        Err(e) => return internal_error(e),
    };
    let ids: Vec<i64> = records.iter().map(|c| c.character_id).collect();
    let chars_query = query.selection(&ids).query();
    render(&CharactersTemplate {
        csrf_token,
        nav: Some(Nav::new(Page::Characters).with_chars_query(chars_query)),
        characters: records
            .into_iter()
            .map(|c| CharacterView {
                id: c.character_id,
                name: c.name,
                needs_relogin: c.needs_relogin,
            })
            .collect(),
        flash: auth::take_flash(&session).await,
    })
}
