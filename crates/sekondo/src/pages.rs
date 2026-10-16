use askama::Template;
use axum::{
    extract::State,
    http::StatusCode,
    response::{Html, IntoResponse, Redirect, Response},
};
use tower_sessions::Session;

use crate::{account::CurrentAccount, app::AppState, auth, csrf};

#[derive(Template)]
#[template(path = "index.html")]
pub struct IndexTemplate {
    pub csrf_token: String,
}

#[derive(Template)]
#[template(path = "error.html")]
pub struct ErrorTemplate {
    pub csrf_token: String,
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
    pub characters: Vec<CharacterView>,
    pub flash: Option<String>,
}

/// The landing page. Signed-in users go straight to their characters.
pub async fn index(account: Option<CurrentAccount>, session: Session) -> Response {
    if account.is_some() {
        return Redirect::to("/characters").into_response();
    }
    match csrf::token(&session).await {
        Ok(csrf_token) => render(&IndexTemplate { csrf_token }),
        Err(e) => internal_error(e),
    }
}

pub async fn characters(
    State(state): State<AppState>,
    account: CurrentAccount,
    session: Session,
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
    render(&CharactersTemplate {
        csrf_token,
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
