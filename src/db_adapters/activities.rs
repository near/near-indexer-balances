use cached::Cached;
use crate::{Balances, models};

use anyhow::Context;
use futures::future::try_join_all;
use futures::SinkExt;
use near_indexer_primitives::views::{ReceiptView, StateChangeCauseView};
use near_client::{Query, ViewClientActor};
use syn::punctuated::Pair;
use crate::models::balance_changes::BalanceChange;


pub(crate) async fn store_activities(
    pool: &sqlx::Pool<sqlx::Postgres>,
    shards: &[near_indexer_primitives::IndexerShard],
    block_header: &near_indexer_primitives::views::BlockHeaderView,
    balances_cache: crate::BalancesCache,
) -> anyhow::Result<()> {
    let futures = shards.iter().map(|shard| {
        // let receipts = shard.chunk.as_ref().map(|chunk| &chunk.receipts).unwrap_or_default();
        store_activities_for_chunk(
            pool,
            &shard.state_changes,
            // &receipts,
            block_header,
            shard.shard_id,
            balances_cache.clone(),
        )
    });

    try_join_all(futures).await.map(|_| ())
}

async fn store_activities_for_chunk(
    pool: &sqlx::Pool<sqlx::Postgres>,
    state_changes: &near_indexer_primitives::views::StateChangesView,
    // receipts: &[near_indexer_primitives::views::ReceiptView],
    block_header: &near_indexer_primitives::views::BlockHeaderView,
    shard_id: near_indexer_primitives::types::ShardId,
    balances_cache: crate::BalancesCache,
) -> anyhow::Result<()> {
    let mut result: Vec<BalanceChange> = vec![];
    for state_change_with_cause in state_changes {
        let near_indexer_primitives::views::StateChangeWithCauseView { cause, value } =
            state_change_with_cause;
        // 99% that we need to look only at 5 types of causes
        // match cause {
        //     // StateChangeCauseView::NotWritableToDisk => {}
        //     // StateChangeCauseView::InitialState => {}
        //     StateChangeCauseView::TransactionProcessing { .. } => {}
        //     // StateChangeCauseView::ActionReceiptProcessingStarted { .. } => {}
        //     StateChangeCauseView::ActionReceiptGasReward { .. } => {}
        //     StateChangeCauseView::ReceiptProcessing { .. } => {}
        //     // StateChangeCauseView::PostponedReceipt { .. } => {}
        //     // StateChangeCauseView::UpdatedDelayedReceipts => {}
        //     StateChangeCauseView::ValidatorAccountsUpdate => {}
        //     StateChangeCauseView::Migration => {}
        //     // StateChangeCauseView::Resharding => {}
        // }
        // But I will try to look at all of them just to be sure we really don't need others
        let (account_id, account): (String, Option<&near_indexer_primitives::views::AccountView>) =
            match value {
                near_indexer_primitives::views::StateChangeValueView::AccountUpdate {
                    account_id,
                    account,
                } => (account_id.to_string(), Some(&account)),
                near_indexer_primitives::views::StateChangeValueView::AccountDeletion {
                    account_id,
                } => (account_id.to_string(), None),
                _ => continue,
            };

        // match cause {
        //     StateChangeCauseView::NotWritableToDisk => {}
        //     StateChangeCauseView::InitialState => {}
        //     StateChangeCauseView::TransactionProcessing { .. } => {}
        //     StateChangeCauseView::ActionReceiptProcessingStarted { .. } => {}
        //     StateChangeCauseView::ActionReceiptGasReward { .. } => {}
        //     StateChangeCauseView::ReceiptProcessing { .. } => {}
        //     StateChangeCauseView::PostponedReceipt { .. } => {}
        //     StateChangeCauseView::UpdatedDelayedReceipts => {}
        //     StateChangeCauseView::ValidatorAccountsUpdate => {}
        //     StateChangeCauseView::Migration => {}
        //     StateChangeCauseView::Resharding => {}
        // }

        result.push(BalanceChange {
            block_timestamp: block_header.timestamp.into(),
            receipt_id: None,
            transaction_hash: None,
            affected_account_id: account_id,
            involved_account_id: "".to_string(),
            direction: "".to_string(),
            delta_liquid_amount: Default::default(),
            absolute_liquid_amount: Default::default(),
            delta_locked_amount: Default::default(),
            absolute_locked_amount: Default::default(),
            shard_id: 0,
            index_in_chunk: 0,


            // account_id,
            // block_timestamp: changed_in_block_timestamp.into(),
            // block_hash: changed_in_block_hash.to_string(),
            // caused_by_transaction_hash: if let near_indexer_primitives::views::StateChangeCauseView::TransactionProcessing { tx_hash } = cause {
            //     Some(tx_hash.to_string())
            // } else {
            //     None
            // },
            // caused_by_receipt_id: match cause {
            //     near_indexer_primitives::views::StateChangeCauseView::ActionReceiptProcessingStarted { receipt_hash } => Some(receipt_hash.to_string()),
            //     near_indexer_primitives::views::StateChangeCauseView::ActionReceiptGasReward { receipt_hash } => Some(receipt_hash.to_string()),
            //     near_indexer_primitives::views::StateChangeCauseView::ReceiptProcessing { receipt_hash } => Some(receipt_hash.to_string()),
            //     near_indexer_primitives::views::StateChangeCauseView::PostponedReceipt { receipt_hash } => Some(receipt_hash.to_string()),
            //     _ => None,
            // },
            // update_reason: cause.print().to_string(),
            // nonstaked_balance: if let Some(acc) = account {
            //     BigDecimal::from_str(acc.amount.to_string().as_str())
            //         .expect("`amount` expected to be u128")
            // } else {
            //     BigDecimal::from(0)
            // },
            // staked_balance: if let Some(acc) = account {
            //     BigDecimal::from_str(acc.locked.to_string().as_str())
            //         .expect("`locked` expected to be u128")
            // } else {
            //     BigDecimal::from(0)
            // },
            // storage_usage: if let Some(acc) = account {
            //     acc.storage_usage.into()
            // } else {
            //     BigDecimal::from(0)
            // },
            // chunk_index_in_block,
            // index_in_chunk
        });
    }

    models::chunked_insert(pool, &result, 10).await?;


    // models::chunked_insert(
    //     pool,
    //     &state_changes
    //         .iter()
    //         .filter_map(|state_change| {
    //             models::account_changes::AccountChange::from_state_change_with_cause(
    //                 state_change,
    //                 block_hash,
    //                 block_timestamp,
    //                 shard_id as i32,
    //                 // we fill it later because we can't enumerate before filtering finishes
    //                 0,
    //             )
    //         })
    //         .enumerate()
    //         .map(|(i, mut account_change)| {
    //             account_change.index_in_chunk = i as i32;
    //             account_change
    //         })
    //         .collect::<Vec<models::account_changes::AccountChange>>(),
    //     10,
    // )
    //     .await?;


    // let action_receipt_actions: Vec<
    //     near_indexer_primitives::views::ReceiptView
    // > = receipts
    //     .iter()
    //     .filter_map(|receipt| {
    //         if let near_indexer_primitives::views::ReceiptEnumView::Action { actions, .. } =
    //         &receipt.receipt
    //         {
    //             Some(actions.iter().map(move |action| {
    //                 models::ActionReceiptAction::from_action_view(
    //                     receipt.receipt_id.to_string(),
    //                     action,
    //                     receipt.predecessor_id.to_string(),
    //                     receipt.receiver_id.to_string(),
    //                     block_hash,
    //                     block_timestamp,
    //                     chunk_header.shard_id as i32,
    //                     // we fill it later because we can't enumerate before filtering finishes
    //                     0,
    //                 )
    //             }))
    //         } else {
    //             None
    //         }
    //     })
    //     .flatten()
    //     .enumerate()
    //     .map(|(i, mut action)| {
    //         action.index_in_chunk = i as i32;
    //         action
    //     })
    //     .collect();

    Ok(())
}

async fn get_previous_balance(account_id: &near_indexer_primitives::types::AccountId, balances_cache: crate::BalancesCache, view_client: &actix::Addr<ViewClientActor>, prev_block_height: u64) -> anyhow::Result<Balances> {
    let mut balances_cache_lock = balances_cache.lock().await;
    let prev_balances = match balances_cache_lock.cache_get(account_id) {
        None => {
            let account_view = get_account_view_for_block_height(view_client, account_id, &prev_block_height).await?;
            let balances = (account_view.amount, account_view.locked);
            balances_cache_lock.cache_set(account_id.clone(), balances);
            balances
        }
        Some(balances) => balances.clone()
    };
    drop(balances_cache_lock);
    Ok(prev_balances)
}

async fn get_account_view_for_block_height(
    view_client: &actix::Addr<ViewClientActor>,
    account_id: &near_indexer_primitives::types::AccountId,
    block_height: &near_indexer_primitives::types::BlockHeight,
) -> anyhow::Result<near_indexer_primitives::views::AccountView> {
    let block_reference = near_indexer_primitives::types::BlockReference::BlockId(
        near_indexer_primitives::types::BlockId::Height(*block_height),
    );
    let request = near_indexer_primitives::views::QueryRequest::ViewAccount {
        account_id: account_id.clone(),
    };
    let query = Query::new(block_reference, request);

    let account_response = view_client
        .send(query)
        .await
        .with_context(|| {
            format!(
                "Failed to deliver ViewAccount for account {}, block {}",
                account_id, block_height
            )
        })?
        .with_context(|| {
            format!(
                "Invalid ViewAccount query for account {}, block {}",
                account_id, block_height
            )
        })?;

    match account_response.kind {
        near_indexer_primitives::views::QueryResponseKind::ViewAccount(account) => Ok(account),
        _ => anyhow::bail!(
            "Failed to extract ViewAccount response for account {}, block {}",
            account_id,
            block_height
        ),
    }
}
