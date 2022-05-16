use crate::{models, Balances};
use cached::Cached;
use std::collections::HashMap;
use std::str::FromStr;

use crate::models::balance_changes::BalanceChange;
use bigdecimal::BigDecimal;
use futures::future::try_join_all;
use near_indexer_primitives::types::AccountId;
use near_indexer_primitives::views::StateChangeCauseView;
use near_jsonrpc_client::errors::JsonRpcError;
use near_jsonrpc_primitives::types::query::RpcQueryError;
use num_traits::Zero;

// https://explorer.near.org/transactions/FGSPpucGQBUTPscfjQRs7Poo4XyaXGawX6QriKbhT3sE#7nu7ZAK3T11erEgG8aWTRGmz9uTHGazoNMjJdVyG3piX

// https://nomicon.io/RuntimeSpec/ApplyingChunk#processing-order
pub(crate) async fn store_balance_changes(
    pool: &sqlx::Pool<sqlx::Postgres>,
    shards: &[near_indexer_primitives::IndexerShard],
    block_header: &near_indexer_primitives::views::BlockHeaderView,
    balances_cache: crate::BalancesCache,
) -> anyhow::Result<()> {
    let futures = shards
        .iter()
        .map(|shard| store_changes_for_chunk(pool, shard, block_header, balances_cache.clone()));

    try_join_all(futures).await.map(|_| ())
}

#[derive(Default)]
struct AccountChangesBalances {
    pub validators: Vec<(AccountId, Balances)>,
    pub transactions: HashMap<near_indexer_primitives::CryptoHash, (AccountId, Balances)>,
    pub receipts: HashMap<near_indexer_primitives::CryptoHash, (AccountId, Balances)>,
    pub rewards: HashMap<near_indexer_primitives::CryptoHash, (AccountId, Balances)>,
}

async fn store_changes_for_chunk(
    pool: &sqlx::Pool<sqlx::Postgres>,
    shard: &near_indexer_primitives::IndexerShard,
    block_header: &near_indexer_primitives::views::BlockHeaderView,
    balances_cache: crate::BalancesCache,
) -> anyhow::Result<()> {
    let mut changes: Vec<BalanceChange> = vec![];
    let mut changes_data = collect_data_from_balance_changes(&shard.state_changes);
    // We should collect these 3 groups sequentially because they all share the same cache
    changes.extend(
        store_validator_accounts_update_for_chunk(
            &changes_data.validators,
            block_header,
            shard.shard_id,
            balances_cache.clone(),
        )
        .await?,
    );
    match shard.chunk.as_ref().map(|chunk| &chunk.transactions) {
        None => {}
        Some(x) => changes.extend(
            store_transaction_execution_outcomes_for_chunk(
                x,
                &mut changes_data.transactions,
                block_header,
                shard.shard_id,
                balances_cache.clone(),
            )
            .await?,
        ),
    }

    changes.extend(
        store_receipt_execution_outcomes_for_chunk(
            &shard.receipt_execution_outcomes,
            &mut changes_data.receipts,
            &mut changes_data.rewards,
            block_header,
            shard.shard_id,
            balances_cache.clone(),
        )
        .await?,
    );

    changes.iter_mut().enumerate().for_each(|(i, mut change)| {
        change.index_in_chunk = i as i32;
    });
    models::chunked_insert(pool, &changes, 10).await?;
    Ok(())
}

fn collect_data_from_balance_changes(
    state_changes: &near_indexer_primitives::views::StateChangesView,
) -> AccountChangesBalances {
    let mut result: AccountChangesBalances = Default::default();

    for state_change_with_cause in state_changes {
        let near_indexer_primitives::views::StateChangeWithCauseView { cause, value } =
            state_change_with_cause;

        let (account_id, balances): (AccountId, Balances) = match value {
            near_indexer_primitives::views::StateChangeValueView::AccountUpdate {
                account_id,
                account,
            } => (account_id.clone(), (account.amount, account.locked)),
            near_indexer_primitives::views::StateChangeValueView::AccountDeletion {
                account_id,
            } => (account_id.clone(), (0, 0)),
            // other values do not provide balance changes
            _ => continue,
        };

        match cause {
            StateChangeCauseView::NotWritableToDisk
            | StateChangeCauseView::InitialState
            | StateChangeCauseView::ActionReceiptProcessingStarted { .. }
            | StateChangeCauseView::UpdatedDelayedReceipts
            | StateChangeCauseView::PostponedReceipt { .. }
            | StateChangeCauseView::Resharding => {
                panic!("We never met that before. It's better to investigate that");
            }
            StateChangeCauseView::ValidatorAccountsUpdate => {
                result.validators.push((account_id, balances));
            }
            StateChangeCauseView::TransactionProcessing { tx_hash } => {
                let a = result.transactions.insert(*tx_hash, (account_id, balances));
                if a.is_some() {
                    panic!(
                        "we have a clash inside 1 block for tx. It's better to investigate that"
                    );
                }
            }
            StateChangeCauseView::Migration {} => {
                // We had this reason once, in block 44337060
                // It does not affect balances, so we can skip it
            }
            StateChangeCauseView::ActionReceiptGasReward { receipt_hash } => {
                result.rewards.insert(*receipt_hash, (account_id, balances));
            }
            StateChangeCauseView::ReceiptProcessing { receipt_hash } => {
                result
                    .receipts
                    .insert(*receipt_hash, (account_id, balances));
            }
        }
    }
    result
}

async fn store_validator_accounts_update_for_chunk(
    validators_balances: &[(AccountId, Balances)],
    block_header: &near_indexer_primitives::views::BlockHeaderView,
    shard_id: near_indexer_primitives::types::ShardId,
    balances_cache: crate::BalancesCache,
) -> anyhow::Result<Vec<BalanceChange>> {
    let mut result: Vec<BalanceChange> = vec![];
    for (account_id, balances) in validators_balances {
        let prev_balances: Balances =
            get_previous_balance(account_id, balances_cache.clone(), block_header.prev_hash)
                .await?;
        let delta_liquid_amount: i128 = (balances.0 as i128) - (prev_balances.0 as i128);
        let delta_locked_amount: i128 = (balances.1 as i128) - (prev_balances.1 as i128);

        set_new_balances(account_id.clone(), *balances, balances_cache.clone()).await;

        result.push(BalanceChange {
            block_timestamp: block_header.timestamp.into(),
            receipt_id: None,
            transaction_hash: None,
            affected_account_id: account_id.to_string(),
            involved_account_id: None,
            direction: "ACTION_TO_AFFECTED_ACCOUNT".to_string(),
            cause: "VALIDATORS_UPDATE".to_string(),
            delta_liquid_amount: BigDecimal::from_str(&delta_liquid_amount.to_string()).unwrap(),
            absolute_liquid_amount: BigDecimal::from_str(&balances.0.to_string()).unwrap(),
            delta_locked_amount: BigDecimal::from_str(&delta_locked_amount.to_string()).unwrap(),
            absolute_locked_amount: BigDecimal::from_str(&balances.1.to_string()).unwrap(),
            shard_id: shard_id as i32,
            // will enumerate later
            index_in_chunk: 0,
        });
    }

    Ok(result)
}

async fn store_transaction_execution_outcomes_for_chunk(
    transactions: &[near_indexer_primitives::IndexerTransactionWithOutcome],
    changes: &mut HashMap<near_indexer_primitives::CryptoHash, (AccountId, Balances)>,
    block_header: &near_indexer_primitives::views::BlockHeaderView,
    shard_id: near_indexer_primitives::types::ShardId,
    balances_cache: crate::BalancesCache,
) -> anyhow::Result<Vec<BalanceChange>> {
    let mut result: Vec<BalanceChange> = vec![];

    for transaction in transactions {
        let affected_account_id = &transaction.transaction.signer_id;
        let prev_balances: Balances = get_previous_balance(
            affected_account_id,
            balances_cache.clone(),
            block_header.prev_hash,
        )
        .await?;

        // todo error messages
        let (account_id, new_balances) = changes
            .remove(&transaction.transaction.hash)
            .expect("should be here");
        // todo
        if account_id != *affected_account_id {
            panic!("should be equal");
        }

        let delta_liquid_amount: i128 = (new_balances.0 as i128) - (prev_balances.0 as i128);
        let delta_locked_amount: i128 = (new_balances.1 as i128) - (prev_balances.1 as i128);

        set_new_balances(account_id.clone(), new_balances, balances_cache.clone()).await;

        // todo I want to rewrite it better
        let involved_account_id = &transaction.transaction.receiver_id;
        let involved_account_id = if *involved_account_id == AccountId::from_str("system")? {
            None
        } else {
            Some(involved_account_id)
        };

        result.push(BalanceChange {
            block_timestamp: block_header.timestamp.into(),
            receipt_id: None,
            transaction_hash: Some(transaction.transaction.hash.to_string()),
            affected_account_id: affected_account_id.to_string(),
            involved_account_id: involved_account_id.map(|id| id.to_string()),
            direction: "ACTION_FROM_AFFECTED_ACCOUNT".to_string(),
            cause: "TRANSACTION_PROCESSING".to_string(),
            delta_liquid_amount: BigDecimal::from_str(&(delta_liquid_amount).to_string()).unwrap(),
            absolute_liquid_amount: BigDecimal::from_str(&new_balances.0.to_string()).unwrap(),
            delta_locked_amount: BigDecimal::from_str(&(delta_locked_amount).to_string()).unwrap(),
            absolute_locked_amount: BigDecimal::from_str(&new_balances.1.to_string()).unwrap(),
            shard_id: shard_id as i32,
            // will enumerate later
            index_in_chunk: 0,
        });

        // ----
        if let Some(account_id) = involved_account_id {
            // balance is not changing here, we just note the line here
            let balances: Balances =
                get_previous_balance(account_id, balances_cache.clone(), block_header.prev_hash)
                    .await?;
            result.push(BalanceChange {
                block_timestamp: block_header.timestamp.into(),
                receipt_id: None,
                transaction_hash: Some(transaction.transaction.hash.to_string()),
                affected_account_id: account_id.to_string(),
                involved_account_id: Some(affected_account_id.to_string()),
                direction: "ACTION_TO_AFFECTED_ACCOUNT".to_string(),
                cause: "TRANSACTION_PROCESSING".to_string(),
                delta_liquid_amount: BigDecimal::zero(),
                absolute_liquid_amount: BigDecimal::from_str(&balances.0.to_string()).unwrap(),
                delta_locked_amount: BigDecimal::zero(),
                absolute_locked_amount: BigDecimal::from_str(&balances.1.to_string()).unwrap(),
                shard_id: shard_id as i32,
                // will enumerate later
                index_in_chunk: 0,
            });
        }
    }
    if !changes.is_empty() {
        panic!("We forgot about some tx.  It's better to investigate that");
    }

    Ok(result)
}

async fn store_receipt_execution_outcomes_for_chunk(
    outcomes_with_receipts: &[near_indexer_primitives::IndexerExecutionOutcomeWithReceipt],
    receipts_changes: &mut HashMap<near_indexer_primitives::CryptoHash, (AccountId, Balances)>,
    rewards_changes: &mut HashMap<near_indexer_primitives::CryptoHash, (AccountId, Balances)>,
    block_header: &near_indexer_primitives::views::BlockHeaderView,
    shard_id: near_indexer_primitives::types::ShardId,
    balances_cache: crate::BalancesCache,
) -> anyhow::Result<Vec<BalanceChange>> {
    let mut result: Vec<BalanceChange> = vec![];

    for outcome_with_receipt in outcomes_with_receipts {
        let affected_account_id = outcome_with_receipt.receipt.predecessor_id.clone();
        let prev_balances: Balances = get_previous_balance(
            &affected_account_id,
            balances_cache.clone(),
            block_header.prev_hash,
        )
        .await?;

        // todo error messages
        let (account_id, new_balances) = receipts_changes
            .remove(&outcome_with_receipt.receipt.receipt_id)
            .expect("should be here");
        if account_id != affected_account_id {
            panic!("should be equal");
        }

        let delta_liquid_amount: i128 = (new_balances.0 as i128) - (prev_balances.0 as i128);
        let delta_locked_amount: i128 = (new_balances.1 as i128) - (prev_balances.1 as i128);

        set_new_balances(
            affected_account_id.clone(),
            new_balances,
            balances_cache.clone(),
        )
        .await;

        // todo I want to rewrite it better
        let involved_account_id = &outcome_with_receipt.receipt.receiver_id;
        let involved_account_id = if involved_account_id == &AccountId::from_str("system")? {
            None
        } else {
            Some(involved_account_id)
        };

        result.push(BalanceChange {
            block_timestamp: block_header.timestamp.into(),
            receipt_id: Some(outcome_with_receipt.receipt.receipt_id.to_string()),
            transaction_hash: None,
            affected_account_id: affected_account_id.to_string(),
            involved_account_id: involved_account_id.map(|id| id.to_string()),
            direction: "ACTION_FROM_AFFECTED_ACCOUNT".to_string(),
            cause: "RECEIPT_PROCESSING".to_string(),
            delta_liquid_amount: BigDecimal::from_str(&(delta_liquid_amount).to_string()).unwrap(),
            absolute_liquid_amount: BigDecimal::from_str(&new_balances.0.to_string()).unwrap(),
            delta_locked_amount: BigDecimal::from_str(&delta_locked_amount.to_string()).unwrap(),
            absolute_locked_amount: BigDecimal::from_str(&new_balances.1.to_string()).unwrap(),
            shard_id: shard_id as i32,
            // will enumerate later
            index_in_chunk: 0,
        });

        if let Some((account_id, new_balances)) =
            rewards_changes.remove(&outcome_with_receipt.receipt.receipt_id)
        {
            let prev_balances: Balances =
                get_previous_balance(&account_id, balances_cache.clone(), block_header.prev_hash)
                    .await?;

            let delta_liquid_amount: i128 = (new_balances.0 as i128) - (prev_balances.0 as i128);
            let delta_locked_amount: i128 = (new_balances.1 as i128) - (prev_balances.1 as i128);

            set_new_balances(account_id.clone(), new_balances, balances_cache.clone()).await;

            result.push(BalanceChange {
                block_timestamp: block_header.timestamp.into(),
                receipt_id: Some(outcome_with_receipt.receipt.receipt_id.to_string()),
                transaction_hash: None,
                affected_account_id: account_id.to_string(),
                involved_account_id: Some(affected_account_id.to_string()),
                direction: "ACTION_TO_AFFECTED_ACCOUNT".to_string(),
                cause: "REWARD".to_string(),
                delta_liquid_amount: BigDecimal::from_str(&(delta_liquid_amount).to_string())
                    .unwrap(),
                absolute_liquid_amount: BigDecimal::from_str(&new_balances.0.to_string()).unwrap(),
                delta_locked_amount: BigDecimal::from_str(&delta_locked_amount.to_string())
                    .unwrap(),
                absolute_locked_amount: BigDecimal::from_str(&new_balances.1.to_string()).unwrap(),
                shard_id: shard_id as i32,
                // will enumerate later
                index_in_chunk: 0,
            });
        }

        // ----
        if let Some(account_id) = involved_account_id {
            // balance is not changing here, we just note the line here
            let balances: Balances =
                get_previous_balance(account_id, balances_cache.clone(), block_header.prev_hash)
                    .await?;
            result.push(BalanceChange {
                block_timestamp: block_header.timestamp.into(),
                receipt_id: Some(outcome_with_receipt.receipt.receipt_id.to_string()),
                transaction_hash: None,
                affected_account_id: account_id.to_string(),
                involved_account_id: Some(affected_account_id.to_string()),
                direction: "ACTION_TO_AFFECTED_ACCOUNT".to_string(),
                cause: "RECEIPT_PROCESSING".to_string(),
                delta_liquid_amount: BigDecimal::zero(),
                absolute_liquid_amount: BigDecimal::from_str(&balances.0.to_string()).unwrap(),
                delta_locked_amount: BigDecimal::zero(),
                absolute_locked_amount: BigDecimal::from_str(&balances.1.to_string()).unwrap(),
                shard_id: shard_id as i32,
                // will enumerate later
                index_in_chunk: 0,
            });

            // and we don't need to put reverse operation for rewards, it makes no sense
        }
    }

    if !receipts_changes.is_empty() {
        panic!("We forgot about some receipts. It's better to investigate that");
    }
    if !rewards_changes.is_empty() {
        panic!("We forgot about some rewards. It's better to investigate that");
    }

    Ok(result)
}

async fn get_previous_balance(
    account_id: &near_indexer_primitives::types::AccountId,
    balances_cache: crate::BalancesCache,
    prev_block_hash: near_indexer_primitives::CryptoHash,
) -> anyhow::Result<Balances> {
    // todo handle 11111111...
    let mut balances_cache_lock = balances_cache.lock().await;
    let prev_balances = match balances_cache_lock.cache_get(account_id) {
        None => {
            let balances = match get_account_view_for_block_hash(account_id, &prev_block_hash).await
            {
                Ok(account_view) => Ok((account_view.amount, account_view.locked)),
                Err(e) => {
                    // If the error has another type, we can't handle it
                    let original_error = e
                        .downcast::<JsonRpcError<RpcQueryError>>()?
                        .handler_error()?;
                    match original_error {
                        // this error means that we try to touch the account which is not created yet
                        // We can safely say that the balance is zero
                        RpcQueryError::UnknownAccount { .. } => Ok((0, 0)),
                        _ => Err(anyhow::Error::new(original_error)),
                    }
                }
            };
            if let Ok(b) = balances {
                balances_cache_lock.cache_set(account_id.clone(), b);
            }
            balances
        }
        Some(balances) => Ok(*balances),
    };
    drop(balances_cache_lock);
    prev_balances
}

async fn set_new_balances(
    account_id: near_indexer_primitives::types::AccountId,
    balances: Balances,
    balances_cache: crate::BalancesCache,
) {
    let mut balances_cache_lock = balances_cache.lock().await;
    balances_cache_lock.cache_set(account_id, balances);
    drop(balances_cache_lock);
}

// todo add retry logic
async fn get_account_view_for_block_hash(
    account_id: &near_indexer_primitives::types::AccountId,
    block_hash: &near_indexer_primitives::CryptoHash,
) -> anyhow::Result<near_indexer_primitives::views::AccountView> {
    let block_reference = near_indexer_primitives::types::BlockReference::BlockId(
        near_indexer_primitives::types::BlockId::Hash(*block_hash),
    );
    let request = near_indexer_primitives::views::QueryRequest::ViewAccount {
        account_id: account_id.clone(),
    };
    let query = near_jsonrpc_client::methods::query::RpcQueryRequest {
        block_reference,
        request,
    };

    // todo
    let a = near_jsonrpc_client::JsonRpcClient::connect("https://archival-rpc.mainnet.near.org");

    let account_response = a.call(query).await?;
    match account_response.kind {
        near_jsonrpc_primitives::types::query::QueryResponseKind::ViewAccount(account) => {
            Ok(account)
        }
        _ => panic!("Unreachable code"),
    }
}
