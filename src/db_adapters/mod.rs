pub(crate) mod account_changes;
pub(crate) mod execution_outcomes;
pub(crate) mod receipts;
pub(crate) mod activities;

pub(crate) const CHUNK_SIZE_FOR_BATCH_INSERT: usize = 100;
