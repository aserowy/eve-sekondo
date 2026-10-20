//! The PostgreSQL implementation of the stores. This is the only code that talks to the database.

mod account;
pub(crate) mod fee;
mod market;
mod opportunity;
pub(crate) mod order;
mod session;
mod station;
pub mod trade;
mod wealth;

use std::sync::Arc;

use secrecy::ExposeSecret;
use sqlx::{PgPool, postgres::PgPoolOptions};

use crate::{
    app::StartupError,
    config::Config,
    store::{StoreError, Stores},
};

use account::PgAccountStore;
use fee::PgFeeStore;
use market::PgMarketStore;
use opportunity::PgOpportunityStore;
use order::PgOrderStore;
use session::PgSessionStore;
use station::PgStationStore;
use trade::PgTradeStore;
use wealth::PgWealthStore;

impl From<sqlx::Error> for StoreError {
    fn from(e: sqlx::Error) -> Self {
        if e.as_database_error()
            .and_then(|db| db.code())
            .is_some_and(|code| code == "23503")
        {
            return Self::MissingParent;
        }
        Self::Backend(e.to_string())
    }
}

/// The stores over one Postgres pool.
#[derive(Clone)]
pub struct PgStores {
    pool: PgPool,
}

impl PgStores {
    /// Connects to the (verify-full) database named in the config and applies pending migrations.
    pub async fn connect(config: &Config) -> Result<Self, StartupError> {
        let pool = PgPoolOptions::new()
            .max_connections(10)
            .connect(config.database_url.expose_secret())
            .await?;
        sqlx::migrate!("./migrations").run(&pool).await?;
        Ok(Self { pool })
    }

    /// Wraps a pool whose migrations are already applied.
    pub fn from_pool(pool: PgPool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> PgPool {
        self.pool.clone()
    }

    pub fn stores(&self) -> Stores {
        Stores {
            accounts: Arc::new(PgAccountStore::new(self.pool.clone())),
            sessions: Arc::new(PgSessionStore::new(self.pool.clone())),
            orders: Arc::new(PgOrderStore::new(self.pool.clone())),
            market: Arc::new(PgMarketStore::new(self.pool.clone())),
            station: Arc::new(PgStationStore::new(self.pool.clone())),
            opportunities: Arc::new(PgOpportunityStore::new(self.pool.clone())),
            wealth: Arc::new(PgWealthStore::new(self.pool.clone())),
            fees: Arc::new(PgFeeStore::new(self.pool.clone())),
            trades: Arc::new(PgTradeStore::new(self.pool.clone())),
        }
    }
}
