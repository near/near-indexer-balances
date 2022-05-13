use std::str::FromStr;

use bigdecimal::BigDecimal;
use sqlx::Arguments;

use crate::models::FieldCount;

#[derive(Debug, sqlx::FromRow, FieldCount)]
pub struct BalanceChange {
    pub block_timestamp: BigDecimal,      // blocks
    pub receipt_id: Option<String>,       // receipts/action_receipt_actions
    pub transaction_hash: Option<String>, // account_changes
    // do we want to additionally store originated_from_transaction_hash?
    pub affected_account_id: String,         // action_receipt_actions
    pub involved_account_id: Option<String>, // action_receipt_actions
    pub direction: String,                   // action_receipt_actions
    pub cause: String,
    pub delta_liquid_amount: BigDecimal, // account_changes + RPC/cache
    pub absolute_liquid_amount: BigDecimal, // account_changes
    pub delta_locked_amount: BigDecimal, // account_changes + RPC/cache
    pub absolute_locked_amount: BigDecimal, // account_changes

    // We definitely want to add the ordering, and, with the understanding of the future interfaces,
    // it should look like that
    pub shard_id: i32, // chunks
    pub index_in_chunk: i32, // natural ordering from S3

                       // pub execution_status: String, // execution_outcomes
                       //
                       // // I suggest to collect both and then maybe drop one of them
                       // // in the example, it was "actions_json". Are we sure we can collapse it to one line?
                       // pub reason: String, // account_changes
                       // pub action_kind: String, // action_receipt_actions
                       //
                       // // We definitely don't want to store a copy of this data second time,
                       // // but maybe it's better place for this info,
                       // // or maybe we want to store a part of this info
                       // pub args: serde_json::Value, // action_receipt_actions
                       //
                       // // do we need this?
                       // pub gas_burnt: BigDecimal, // execution_outcomes
                       // pub tokens_burnt: BigDecimal, // execution_outcomes
}

impl crate::models::SqlxMethods for BalanceChange {
    fn add_to_args(&self, args: &mut sqlx::postgres::PgArguments) {
        args.add(&self.block_timestamp);
        args.add(&self.receipt_id);
        args.add(&self.transaction_hash);
        args.add(&self.affected_account_id);
        args.add(&self.involved_account_id);
        args.add(&self.direction);
        args.add(&self.delta_liquid_amount);
        args.add(&self.absolute_liquid_amount);
        args.add(&self.delta_locked_amount);
        args.add(&self.absolute_locked_amount);
        args.add(&self.shard_id);
        args.add(&self.index_in_chunk);
    }

    fn insert_query(count: usize) -> anyhow::Result<String> {
        crate::models::create_query_with_placeholders(
            "INSERT INTO balance_changes VALUES",
            count,
            BalanceChange::field_count(),
        )
    }

    fn name() -> String {
        "balance_changes".to_string()
    }
}
