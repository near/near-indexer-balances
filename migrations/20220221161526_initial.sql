CREATE TABLE balance_changes
(
    block_timestamp           numeric(20, 0) NOT NULL,
    receipt_id                text,
    transaction_hash          text,
    affected_account_id       text           NOT NULL,
    involved_account_id       text,
    direction                 text           NOT NULL,
    cause                     text           NOT NULL,
    status                    text           NOT NULL,
    delta_nonstaked_amount    numeric(45, 0) NOT NULL,
    absolute_nonstaked_amount numeric(45, 0) NOT NULL,
    delta_staked_amount       numeric(45, 0) NOT NULL,
    absolute_staked_amount    numeric(45, 0) NOT NULL,
    shard_id                  integer        NOT NULL,
    index_in_chunk            integer        NOT NULL,
    PRIMARY KEY (block_timestamp, shard_id, index_in_chunk)
);

CREATE INDEX CONCURRENTLY balance_changes_affected_account_idx ON balance_changes (affected_account_id);
CREATE INDEX CONCURRENTLY balance_changes_receipt_id_idx ON balance_changes (receipt_id);

-- ALTER TABLE balance_changes
--     ADD CONSTRAINT balance_changes_receipt_id_fk FOREIGN KEY (receipt_id) REFERENCES action_receipts(receipt_id);
-- ALTER TABLE balance_changes
--     ADD CONSTRAINT balance_changes_tx_hash_fk FOREIGN KEY (transaction_hash) REFERENCES transactions(transaction_hash);
