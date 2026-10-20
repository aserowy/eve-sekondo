//! Runs every contract case against a freshly migrated PostgreSQL with TimescaleDB and against the
//! in-memory store. Only when all of them pass does it record the store fingerprint.

use std::{process::ExitCode, str::FromStr, sync::Arc};

use sekondo::{
    clock::SystemClock,
    config::check_database_url,
    store::{memory::MemoryStores, pg::PgStores},
};
use sqlx::{
    Connection, PgConnection, PgPool,
    postgres::{PgConnectOptions, PgPoolOptions},
};
use store_contract::{
    cases,
    gate::workspace_root,
    verify::{Failure, record_if_clean, run_case},
};
use time::OffsetDateTime;

const DATABASE: &str = "sekondo_store_verify";

#[tokio::main]
async fn main() -> ExitCode {
    sekondo::tls::install_crypto_provider();
    match verify().await {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

async fn verify() -> Result<bool, Box<dyn std::error::Error>> {
    let _ = dotenvy::dotenv();
    let url = std::env::var("DATABASE_URL").map_err(|_| "DATABASE_URL is not set")?;
    check_database_url(&url)?;
    let options = PgConnectOptions::from_str(&url)?;

    // A dedicated database, so a verification never touches the app's own data.
    let mut admin = PgConnection::connect_with(&options).await?;
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "DROP DATABASE IF EXISTS {DATABASE} WITH (FORCE)"
    )))
    .execute(&mut admin)
    .await?;
    sqlx::query(sqlx::AssertSqlSafe(format!("CREATE DATABASE {DATABASE}")))
        .execute(&mut admin)
        .await?;
    admin.close().await?;

    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect_with(options.database(DATABASE))
        .await?;
    sqlx::migrate!("../sekondo/migrations").run(&pool).await?;

    let cases = cases::all();
    let postgres = PgStores::from_pool(pool.clone()).stores();
    let mut failures: Vec<Failure> = Vec::new();
    for case in &cases {
        reset(&pool).await?;
        if let Err(failure) = run_case("postgres", case, &postgres).await {
            failures.push(failure);
        }
        let memory = MemoryStores::new(Arc::new(SystemClock)).stores();
        if let Err(failure) = run_case("memory", case, &memory).await {
            failures.push(failure);
        }
    }

    for failure in &failures {
        eprintln!(
            "FAILED {} on {}:\n{}\n",
            failure.case, failure.store, failure.message
        );
    }
    let wrote = record_if_clean(
        &workspace_root(),
        &failures,
        &cases,
        OffsetDateTime::now_utc(),
    )?;
    if wrote {
        println!(
            "verified {} cases; recorded verified-store.json",
            cases.len()
        );
    } else {
        eprintln!(
            "{} of {} cases failed; verified-store.json is unchanged",
            failures
                .iter()
                .map(|f| f.case.as_str())
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            cases.len()
        );
    }
    Ok(wrote)
}

/// Empties every table and the rollups, so each case starts from an empty store. The faction
/// table is data the migrations load, not state a case changes.
async fn reset(pool: &PgPool) -> Result<(), sqlx::Error> {
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT tablename FROM pg_tables
         WHERE schemaname = 'public'
           AND tablename NOT IN ('_sqlx_migrations', 'npc_corporation_factions')",
    )
    .fetch_all(pool)
    .await?;
    if !tables.is_empty() {
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "TRUNCATE {} RESTART IDENTITY CASCADE",
            tables.join(", ")
        )))
        .execute(pool)
        .await?;
    }
    for view in ["station_hourly", "station_daily"] {
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "CALL refresh_continuous_aggregate('{view}', NULL, NULL)"
        )))
        .execute(pool)
        .await?;
    }
    Ok(())
}
