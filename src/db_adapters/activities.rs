use crate::models;

use futures::future::try_join_all;


pub(crate) async fn store_activities(
    pool: &sqlx::Pool<sqlx::Postgres>,
    shards: &[near_indexer_primitives::IndexerShard],
    block_hash: &near_indexer_primitives::CryptoHash,
    block_timestamp: u64,
) -> anyhow::Result<()> {
    let futures = shards.iter().map(|shard| {
        store_account_changes_for_chunk(
            pool,
            &shard.state_changes,
            &shard.chunk.map_or(&[], |chunk| &chunk.receipts),
            block_hash,
            block_timestamp,
            shard.shard_id,
        )
    });

    try_join_all(futures).await.map(|_| ())
}

async fn store_activities_for_chunk(
    pool: &sqlx::Pool<sqlx::Postgres>,
    state_changes: &near_indexer_primitives::views::StateChangesView,
    receipts: &[near_indexer_primitives::views::ReceiptView],
    block_hash: &near_indexer_primitives::CryptoHash,
    block_timestamp: u64,
    shard_id: near_indexer_primitives::types::ShardId,
) -> anyhow::Result<()> {
    models::chunked_insert(
        pool,
        &state_changes
            .iter()
            .filter_map(|state_change| {
                models::account_changes::AccountChange::from_state_change_with_cause(
                    state_change,
                    block_hash,
                    block_timestamp,
                    shard_id as i32,
                    // we fill it later because we can't enumerate before filtering finishes
                    0,
                )
            })
            .enumerate()
            .map(|(i, mut account_change)| {
                account_change.index_in_chunk = i as i32;
                account_change
            })
            .collect::<Vec<models::account_changes::AccountChange>>(),
        10,
    )
        .await?;





    let action_receipt_actions: Vec<
        near_indexer_primitives::views::ReceiptView
    > = receipts
        .iter()
        .filter_map(|receipt| {
            if let near_indexer_primitives::views::ReceiptEnumView::Action { actions, .. } =
            &receipt.receipt
            {
                Some(actions.iter().map(move |action| {
                    models::ActionReceiptAction::from_action_view(
                        receipt.receipt_id.to_string(),
                        action,
                        receipt.predecessor_id.to_string(),
                        receipt.receiver_id.to_string(),
                        block_hash,
                        block_timestamp,
                        chunk_header.shard_id as i32,
                        // we fill it later because we can't enumerate before filtering finishes
                        0,
                    )
                }))
            } else {
                None
            }
        })
        .flatten()
        .enumerate()
        .map(|(i, mut action)| {
            action.index_in_chunk = i as i32;
            action
        })
        .collect();

    models::chunked_insert(pool, &action_receipt_actions, 10,).await?;

    Ok(())
}
