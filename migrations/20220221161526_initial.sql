CREATE TYPE direction AS ENUM (
    'ACTION_FROM_AFFECTED_ACCOUNT',
    'ACTION_TO_AFFECTED_ACCOUNT',
    'NONE'
);

CREATE TABLE balance_changes
(
    block_timestamp        numeric(20, 0) NOT NULL,
    receipt_id             text,
    transaction_hash       text,
    affected_account_id    text           NOT NULL,
    involved_account_id    text,
    direction              direction      NOT NULL,
    cause                  text           NOT NULL,
    delta_liquid_amount    numeric(45, 0) NOT NULL,
    absolute_liquid_amount numeric(45, 0) NOT NULL,
    delta_locked_amount    numeric(45, 0) NOT NULL,
    absolute_locked_amount numeric(45, 0) NOT NULL,
    shard_id               integer        NOT NULL,
    index_in_chunk         integer        NOT NULL,
    PRIMARY KEY (block_timestamp, shard_id, index_in_chunk)
);

-- TODO maybe we need to change primary key
-- TODO add indexes
