// // TODO cleanup imports in all the files in the end
use cached::SizedCache;
use clap::Parser;
use futures::StreamExt;
use near_primitives::time::Utc;

use metrics_server::{
    init_metrics_server, BLOCK_PROCESSED_TOTAL, LAST_SEEN_BLOCK_HEIGHT, LATEST_BLOCK_TIMESTAMP_DIFF,
};
use near_lake_framework::near_indexer_primitives;
use near_primitives::utils::from_timestamp;
use tokio::sync::Mutex;
use tracing_subscriber::EnvFilter;

mod configs;
mod db_adapters;
mod metrics_server;
mod models;

#[macro_use]
extern crate lazy_static;

// TODO naming
pub(crate) const LOGGING_PREFIX: &str = "indexer_balances";

const INTERVAL: std::time::Duration = std::time::Duration::from_millis(100);
const MAX_DELAY_TIME: std::time::Duration = std::time::Duration::from_secs(120);
const RETRY_COUNT: usize = 10;

#[derive(Debug, Default, Clone, Copy)]
pub struct BalanceDetails {
    pub non_staked: near_indexer_primitives::types::Balance,
    pub staked: near_indexer_primitives::types::Balance,
}

#[derive(Debug, Clone)]
pub struct AccountWithBalance {
    pub account_id: near_indexer_primitives::types::AccountId,
    pub balance: BalanceDetails,
}

pub type BalanceCache =
    std::sync::Arc<Mutex<SizedCache<near_indexer_primitives::types::AccountId, BalanceDetails>>>;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();

    let opts = crate::configs::Opts::parse();
    let pool = sqlx::PgPool::connect(&std::env::var("DATABASE_URL")?).await?;
    // TODO Error: while executing migrations: error returned from database: 1128 (HY000): Function 'near_indexer.GET_LOCK' is not defined
    // sqlx::migrate!().run(&pool).await?;

    let start_block_height = match opts.start_block_height {
        Some(x) => x,
        None => models::start_after_interruption(&pool).await?,
    };
    let config_builder = near_lake_framework::LakeConfigBuilder::default();

    let config = match opts.chain_id.as_str() {
        "mainnet" => config_builder.mainnet(),
        "testnet" => config_builder.testnet(),
        _ => panic!(),
    }
    .start_block_height(start_block_height)
    .build()?;

    init_tracing();

    let (_lake_handle, stream) = near_lake_framework::streamer(config);

    // We want to prevent unnecessary RPC queries to find previous balance
    let balances_cache: BalanceCache =
        std::sync::Arc::new(Mutex::new(SizedCache::with_size(100_000)));

    let json_rpc_client = near_jsonrpc_client::JsonRpcClient::connect(&opts.near_archival_rpc_url);
    tokio::spawn(async move {
        let mut handlers = tokio_stream::wrappers::ReceiverStream::new(stream)
            .map(|streamer_message| {
                handle_streamer_message(streamer_message, &pool, &balances_cache, &json_rpc_client)
            })
            .buffer_unordered(1usize);

        let mut time_now = std::time::Instant::now();
        while let Some(handle_message) = handlers.next().await {
            match handle_message {
                Ok(block_height) => {
                    let elapsed = time_now.elapsed();
                    tracing::trace!(
                        "Elapsed time spent on block {}: {:.3?}",
                        block_height,
                        elapsed
                    );
                    time_now = std::time::Instant::now();
                }
                Err(_e) => {
                    // return Err(anyhow::Error!(e));
                }

            }
    }
    });
    // });
    init_metrics_server().await?;

    Ok(())
}

async fn handle_streamer_message(
    streamer_message: near_indexer_primitives::StreamerMessage,
    pool: &sqlx::Pool<sqlx::Postgres>,
    balances_cache: &BalanceCache,
    json_rpc_client: &near_jsonrpc_client::JsonRpcClient,
) -> anyhow::Result<u64> {
    LAST_SEEN_BLOCK_HEIGHT.set(streamer_message.block.header.height.try_into().unwrap());
    let now = Utc::now();
    let block_timestamp = from_timestamp(streamer_message.block.header.timestamp_nanosec);
    LATEST_BLOCK_TIMESTAMP_DIFF.set((now - block_timestamp).num_seconds() as f64);

    db_adapters::balance_changes::store_balance_changes(
        pool,
        &streamer_message.shards,
        &streamer_message.block.header,
        balances_cache,
        json_rpc_client,
    )
    .await?;

    BLOCK_PROCESSED_TOTAL.inc();
    Ok(streamer_message.block.header.height)
}

fn init_tracing() {
    let mut env_filter = EnvFilter::new("near_lake_framework=info");

    if let Ok(rust_log) = std::env::var("RUST_LOG") {
        if !rust_log.is_empty() {
            for directive in rust_log.split(',').filter_map(|s| match s.parse() {
                Ok(directive) => Some(directive),
                Err(err) => {
                    eprintln!("Ignoring directive `{}`: {}", s, err);
                    None
                }
            }) {
                env_filter = env_filter.add_directive(directive);
            }
        }
    }

    tracing_subscriber::fmt::Subscriber::builder()
        .with_env_filter(env_filter)
        .with_writer(std::io::stderr)
        .init();
}
