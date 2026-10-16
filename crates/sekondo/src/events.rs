//! Change signals for open pages. The bus sits behind a trait so a multi-instance deployment can
//! swap in Postgres `LISTEN/NOTIFY` without touching publishers or the SSE endpoint.

use futures_util::{StreamExt, stream::BoxStream};
use tokio::sync::broadcast;
use uuid::Uuid;

/// What changed. Signals carry no data: a page that gets one re-fetches the affected parts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    OrdersUpdated,
}

impl Event {
    /// The SSE event name.
    pub fn name(self) -> &'static str {
        match self {
            Self::OrdersUpdated => "orders-updated",
        }
    }
}

pub trait EventBus: Send + Sync {
    fn publish(&self, account: Uuid, event: Event);
    /// Signals for one account only. A subscriber that fell behind gets
    /// [`Event::OrdersUpdated`] instead of an error: re-fetching is idempotent.
    fn subscribe(&self, account: Uuid) -> BoxStream<'static, Event>;
}

const CAPACITY: usize = 256;

pub struct InProcessBus {
    tx: broadcast::Sender<(Uuid, Event)>,
}

impl InProcessBus {
    pub fn new() -> Self {
        Self::with_capacity(CAPACITY)
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            tx: broadcast::channel(capacity).0,
        }
    }
}

impl Default for InProcessBus {
    fn default() -> Self {
        Self::new()
    }
}

impl EventBus for InProcessBus {
    fn publish(&self, account: Uuid, event: Event) {
        // No subscribers is the normal case when nobody has a page open.
        let _ = self.tx.send((account, event));
    }

    fn subscribe(&self, account: Uuid) -> BoxStream<'static, Event> {
        futures_util::stream::unfold(self.tx.subscribe(), move |mut rx| async move {
            loop {
                match rx.recv().await {
                    Ok((who, event)) if who == account => return Some((event, rx)),
                    Ok(_) => {}
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        return Some((Event::OrdersUpdated, rx));
                    }
                    Err(broadcast::error::RecvError::Closed) => return None,
                }
            }
        })
        .boxed()
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    async fn next(stream: &mut BoxStream<'static, Event>) -> Option<Event> {
        tokio::time::timeout(Duration::from_millis(100), stream.next())
            .await
            .ok()
            .flatten()
    }

    #[tokio::test]
    async fn a_signal_reaches_only_the_subscribers_of_its_account() {
        let bus = InProcessBus::new();
        let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
        let mut stream_a = bus.subscribe(a);
        let mut stream_a2 = bus.subscribe(a);
        let mut stream_b = bus.subscribe(b);

        bus.publish(a, Event::OrdersUpdated);

        assert_eq!(next(&mut stream_a).await, Some(Event::OrdersUpdated));
        assert_eq!(next(&mut stream_a2).await, Some(Event::OrdersUpdated));
        assert_eq!(next(&mut stream_b).await, None);
    }

    #[tokio::test]
    async fn publishing_without_subscribers_is_fine() {
        InProcessBus::new().publish(Uuid::new_v4(), Event::OrdersUpdated);
    }

    #[tokio::test]
    async fn a_lagging_subscriber_gets_a_signal_instead_of_an_error() {
        let bus = InProcessBus::with_capacity(2);
        let account = Uuid::new_v4();
        let mut stream = bus.subscribe(account);

        // Many signals for other accounts overflow the buffer before the stream is read.
        for _ in 0..10 {
            bus.publish(Uuid::new_v4(), Event::OrdersUpdated);
        }

        assert_eq!(next(&mut stream).await, Some(Event::OrdersUpdated));
        // After catching up it filters again.
        assert_eq!(next(&mut stream).await, None);
    }

    #[test]
    fn the_sse_name_is_stable() {
        assert_eq!(Event::OrdersUpdated.name(), "orders-updated");
    }
}
