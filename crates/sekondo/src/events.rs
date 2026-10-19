//! Change signals for open pages. The bus sits behind a trait so a multi-instance deployment can
//! swap in Postgres `LISTEN/NOTIFY` without touching publishers or the SSE endpoint.

use futures_util::{StreamExt, stream::BoxStream};
use tokio::sync::broadcast;
use uuid::Uuid;

/// What changed. Signals carry no data: a page that gets one re-fetches the affected parts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    OrdersUpdated,
    /// A market refresh changed the standing or the gap of an order. Only the orders page
    /// listens to it.
    MarketUpdated,
    /// A wallet or assets poll, or a price refresh, changed what a character is worth. The wealth
    /// views listen to it and to `OrdersUpdated`, which is also what a lagging subscriber gets.
    WealthUpdated,
    /// A skills or standings poll, or the lookup of a station owner, changed a fee of a
    /// character. Only the orders page's character entries listen to it.
    FeesUpdated,
    /// The station book was refreshed or a history pass stored rows. Sent to every account: the
    /// station market is public. Only the opportunities page listens to it.
    OpportunitiesUpdated,
    /// A transactions poll stored a new purchase or sale, so the trades page has a new row.
    TradesUpdated,
}

impl Event {
    /// The SSE event name.
    pub fn name(self) -> &'static str {
        match self {
            Self::OrdersUpdated => "orders-updated",
            Self::MarketUpdated => "market-updated",
            Self::WealthUpdated => "wealth-updated",
            Self::FeesUpdated => "fees-updated",
            Self::OpportunitiesUpdated => "opportunities-updated",
            Self::TradesUpdated => "trades-updated",
        }
    }
}

pub trait EventBus: Send + Sync {
    fn publish(&self, account: Uuid, event: Event);
    /// A signal for every account, for what is public. Reaches every subscriber once.
    fn publish_all(&self, event: Event);
    /// Signals for one account only. A subscriber that fell behind gets
    /// [`Event::OrdersUpdated`] instead of an error: re-fetching is idempotent.
    fn subscribe(&self, account: Uuid) -> BoxStream<'static, Event>;
}

const CAPACITY: usize = 256;

/// Who a signal is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    Account(Uuid),
    All,
}

pub struct InProcessBus {
    tx: broadcast::Sender<(Target, Event)>,
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
        let _ = self.tx.send((Target::Account(account), event));
    }

    fn publish_all(&self, event: Event) {
        let _ = self.tx.send((Target::All, event));
    }

    fn subscribe(&self, account: Uuid) -> BoxStream<'static, Event> {
        futures_util::stream::unfold(self.tx.subscribe(), move |mut rx| async move {
            loop {
                match rx.recv().await {
                    Ok((Target::All, event)) => return Some((event, rx)),
                    Ok((Target::Account(who), event)) if who == account => {
                        return Some((event, rx));
                    }
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
    fn the_sse_names_are_stable() {
        assert_eq!(Event::OrdersUpdated.name(), "orders-updated");
        assert_eq!(Event::MarketUpdated.name(), "market-updated");
        assert_eq!(Event::WealthUpdated.name(), "wealth-updated");
    }

    #[tokio::test]
    async fn a_wealth_signal_reaches_only_its_account() {
        let bus = InProcessBus::new();
        let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
        let mut stream_a = bus.subscribe(a);
        let mut stream_b = bus.subscribe(b);

        bus.publish(a, Event::WealthUpdated);

        assert_eq!(next(&mut stream_a).await, Some(Event::WealthUpdated));
        assert_eq!(next(&mut stream_b).await, None);
    }

    #[tokio::test]
    async fn a_lagging_subscriber_that_missed_wealth_signals_still_gets_the_orders_signal() {
        let bus = InProcessBus::with_capacity(2);
        let account = Uuid::new_v4();
        let mut stream = bus.subscribe(account);

        for _ in 0..10 {
            bus.publish(Uuid::new_v4(), Event::WealthUpdated);
        }

        // Every wealth view also listens to the orders signal.
        assert_eq!(next(&mut stream).await, Some(Event::OrdersUpdated));
    }

    #[tokio::test]
    async fn a_market_signal_reaches_only_its_account() {
        let bus = InProcessBus::new();
        let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
        let mut stream_a = bus.subscribe(a);
        let mut stream_b = bus.subscribe(b);

        bus.publish(a, Event::MarketUpdated);

        assert_eq!(next(&mut stream_a).await, Some(Event::MarketUpdated));
        assert_eq!(next(&mut stream_b).await, None);
    }

    #[tokio::test]
    async fn a_broadcast_reaches_every_account() {
        let bus = InProcessBus::new();
        let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
        let mut stream_a = bus.subscribe(a);
        let mut stream_b = bus.subscribe(b);
        let mut stream_a2 = bus.subscribe(a);

        bus.publish_all(Event::OpportunitiesUpdated);

        assert_eq!(next(&mut stream_a).await, Some(Event::OpportunitiesUpdated));
        assert_eq!(next(&mut stream_b).await, Some(Event::OpportunitiesUpdated));
        assert_eq!(
            next(&mut stream_a2).await,
            Some(Event::OpportunitiesUpdated)
        );
        assert_eq!(next(&mut stream_a).await, None, "once");
    }

    #[tokio::test]
    async fn a_broadcast_and_an_account_signal_do_not_get_mixed_up() {
        let bus = InProcessBus::new();
        let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
        let mut stream_a = bus.subscribe(a);
        let mut stream_b = bus.subscribe(b);

        bus.publish(a, Event::OrdersUpdated);
        bus.publish_all(Event::OpportunitiesUpdated);

        assert_eq!(next(&mut stream_a).await, Some(Event::OrdersUpdated));
        assert_eq!(next(&mut stream_a).await, Some(Event::OpportunitiesUpdated));
        assert_eq!(next(&mut stream_b).await, Some(Event::OpportunitiesUpdated));
        assert_eq!(next(&mut stream_b).await, None, "b never saw a's signal");
    }

    #[test]
    fn the_opportunities_signal_has_its_own_stable_name() {
        assert_eq!(Event::OpportunitiesUpdated.name(), "opportunities-updated");
    }

    #[tokio::test]
    async fn the_lag_fallback_is_still_the_orders_signal_whatever_was_missed() {
        let bus = InProcessBus::with_capacity(2);
        let account = Uuid::new_v4();
        let mut stream = bus.subscribe(account);

        for _ in 0..10 {
            bus.publish(Uuid::new_v4(), Event::MarketUpdated);
        }

        assert_eq!(next(&mut stream).await, Some(Event::OrdersUpdated));
    }
}
