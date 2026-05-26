-- Minimal legacy `katpool_mainnet` schema.
-- Mirrors the production schema captured by `\d+` in
-- docs/db-schema.md's "Legacy schema reference" snapshot.
-- Used by integration tests that exercise the importer end-to-end.

CREATE TABLE IF NOT EXISTS block_details (
    mined_block_hash  varchar(255) PRIMARY KEY,
    miner_id          varchar(255),
    pool_address      varchar(255),
    wallet            varchar(255),
    daa_score         varchar(255),
    timestamp         timestamp without time zone DEFAULT now(),
    reward_block_hash varchar(255) DEFAULT '',
    miner_reward      bigint NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS miners_balance (
    id                varchar(255) PRIMARY KEY,
    miner_id          varchar(255),
    wallet            varchar(255),
    balance           numeric,
    nacho_rebate_kas  numeric DEFAULT 0
);

CREATE TABLE IF NOT EXISTS payments (
    id               serial PRIMARY KEY,
    wallet_address   text[] NOT NULL,
    amount           bigint NOT NULL,
    timestamp        timestamp without time zone DEFAULT now(),
    transaction_hash varchar(255) NOT NULL
);

CREATE TABLE IF NOT EXISTS nacho_payments (
    id               serial PRIMARY KEY,
    wallet_address   text[] NOT NULL,
    nacho_amount     bigint NOT NULL,
    timestamp        timestamp without time zone DEFAULT now(),
    transaction_hash varchar(255) NOT NULL
);

DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_type WHERE typname = 'status_enum') THEN
        CREATE TYPE status_enum AS ENUM ('PENDING', 'COMPLETED', 'FAILED');
    END IF;
END$$;

CREATE TABLE IF NOT EXISTS pending_krc20_transfers (
    id                    serial PRIMARY KEY,
    first_txn_id          varchar(255) NOT NULL UNIQUE,
    sompi_to_miner        bigint NOT NULL,
    nacho_amount          bigint NOT NULL,
    address               varchar(255) NOT NULL,
    p2sh_address          varchar(255) NOT NULL,
    nacho_transfer_status status_enum DEFAULT 'PENDING',
    db_entry_status       status_enum DEFAULT 'PENDING',
    timestamp             timestamp without time zone DEFAULT now()
);

CREATE TABLE IF NOT EXISTS wallet_total (
    address varchar(255) PRIMARY KEY,
    total   numeric
);
