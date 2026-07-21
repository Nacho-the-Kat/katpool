-- Active-miners history (`COUNT DISTINCT wallet_id` over `credited_at` ranges)
-- heap-fetches every share row when `wallet_id` is not in the covering index.
-- That pushes the default Overview 7d series past the dashboard BFF's upstream
-- timeout (~8s), surfacing as a 502 on `/api/v1/pool/miners/history`.
--
-- Mirror the hashrate INCLUDE (difficulty) pattern so both pool-wide series
-- can use an index-only scan.
DROP INDEX IF EXISTS idx_share_credited_at;
CREATE INDEX idx_share_credited_at
    ON share (credited_at)
    INCLUDE (difficulty, wallet_id);
