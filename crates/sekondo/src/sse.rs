//! `GET /events`: a server-sent event stream of change signals for the signed-in account.

use std::{convert::Infallible, time::Duration};

use axum::{
    extract::State,
    http::StatusCode,
    response::{
        IntoResponse, Response,
        sse::{Event as SseEvent, Sse},
    },
};
use futures_util::{StreamExt, stream::BoxStream};
use tokio::time::Interval;
use tower_sessions::{Session, session::Id};

use crate::{account, app::AppState, events::Event, session};

/// A keep-alive goes out this often, and the session is re-checked each time, so a stream ends
/// within one interval of its session ending. Well inside the 60 seconds the spec allows.
pub const KEEP_ALIVE: Duration = Duration::from_secs(15);

struct StreamState {
    signals: BoxStream<'static, Event>,
    ticker: Interval,
    state: AppState,
    session_id: Id,
    first: bool,
}

/// 401 without a signed-in session (an `EventSource` would follow a redirect to an HTML page).
pub async fn stream(State(state): State<AppState>, session: Session) -> Response {
    let (Some(account_id), Some(session_id)) =
        (account::current_account_id(&session).await, session.id())
    else {
        return StatusCode::UNAUTHORIZED.into_response();
    };

    let mut ticker = tokio::time::interval(state.sse_tick);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let initial = StreamState {
        signals: state.events.subscribe(account_id),
        ticker,
        state,
        session_id,
        first: true,
    };

    let events = futures_util::stream::unfold(initial, |mut st| async move {
        // Opens the response right away, so clients see the stream is up.
        if std::mem::take(&mut st.first) {
            st.ticker.reset();
            return Some((
                Ok::<_, Infallible>(SseEvent::default().comment("connected")),
                st,
            ));
        }
        tokio::select! {
            signal = st.signals.next() => {
                let signal = signal?;
                // The data is the signal's name again: an event without data is not dispatched.
                let event = SseEvent::default().event(signal.name()).data(signal.name());
                Some((Ok(event), st))
            }
            _ = st.ticker.tick() => {
                let store = session::PgSessionStore::new(st.state.pool.clone());
                if !session::is_active(&store, &st.session_id, st.state.clock.now()).await {
                    return None;
                }
                Some((Ok(SseEvent::default().comment("keep-alive")), st))
            }
        }
    });
    Sse::new(events).into_response()
}
