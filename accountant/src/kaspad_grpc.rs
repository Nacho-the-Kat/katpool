//! Real [`KaspadClient`] implementation backed by `kaspa-grpc-client`.
//!
//! Translates the tracker's two-method abstract surface into
//! kaspad gRPC calls:
//!
//! - [`KaspadClient::get_virtual_blue_score`] → [`RpcApi::get_sink_blue_score`]
//! - [`KaspadClient::get_block`]              → [`RpcApi::get_block_call`]
//!   with `include_transactions: true`, then extracts
//!   blue / DAA / coinbase-reward from the returned `RpcBlock`.
//!
//! ## Coinbase reward extraction
//!
//! The configured `pool_addresses` are the kaspa addresses whose
//! coinbase outputs count as pool revenue. The reward sompi is
//! the **sum of `value`** across every output in the coinbase
//! transaction whose `verbose_data.script_public_key_address`
//! matches one of those addresses. Addresses are compared by
//! full equality (prefix + payload).
//!
//! kaspad populates `verbose_data` on transactions when the
//! caller asks for them via `include_transactions: true`. Each
//! `RpcBlock` also carries a `verbose_data` on the block itself —
//! we read `blue_score` and `is_chain_block` from it. If either
//! verbose-data field is missing we surface
//! `KaspadError::Malformed` rather than silently fabricating
//! defaults.
//!
//! ## Block-not-found vs. transport error
//!
//! kaspad returns an `RpcError` for unknown block hashes. The
//! exact message varies by kaspad version, so we match
//! defensively on the `BlockNotFound` variant. Anything else
//! becomes [`KaspadError::Transport`].

use std::sync::Arc;

use async_trait::async_trait;
use kaspa_addresses::Address;
use kaspa_grpc_client::GrpcClient;
use kaspa_hashes::Hash as KaspaHash;
use kaspa_rpc_core::api::rpc::RpcApi;
use kaspa_rpc_core::model::message::GetBlockRequest;
use kaspa_rpc_core::{RpcBlock, RpcError};

use katpool_domain::BlockHash;
use tracing::{debug, warn};

use crate::maturity::{BlockInfo, KaspadClient, KaspadError};

/// Backing client for the real-kaspad implementation.
pub struct KaspadGrpcClient {
    client: Arc<GrpcClient>,
    pool_addresses: Vec<Address>,
}

impl KaspadGrpcClient {
    /// Construct from an already-connected `GrpcClient`. The
    /// caller is responsible for connection lifecycle (rusty-
    /// kaspa's `GrpcClient` exposes `connect_with_args(...)` plus
    /// reconnect callbacks; wiring that is the operator binary's
    /// job, not this module's).
    ///
    /// `pool_addresses` is the address set whose coinbase outputs
    /// count as pool revenue. Pass exactly one address for the
    /// common case; supports multiple for future multi-address
    /// pool setups (e.g. hot/cold split).
    #[must_use]
    pub const fn new(client: Arc<GrpcClient>, pool_addresses: Vec<Address>) -> Self {
        Self {
            client,
            pool_addresses,
        }
    }

    /// Read-only access to the configured pool addresses
    /// (operator-visible diagnostic).
    #[must_use]
    pub fn pool_addresses(&self) -> &[Address] {
        &self.pool_addresses
    }
}

#[async_trait]
impl KaspadClient for KaspadGrpcClient {
    async fn get_virtual_blue_score(&self) -> Result<u64, KaspadError> {
        self.client
            .get_sink_blue_score()
            .await
            .map_err(|e| map_rpc_error_to_transport(&e))
    }

    async fn get_block(&self, hash: BlockHash) -> Result<Option<BlockInfo>, KaspadError> {
        let kaspa_hash = KaspaHash::from_bytes(*hash.as_bytes());
        let request = GetBlockRequest::new(kaspa_hash, true);
        match self.client.get_block_call(None, request).await {
            Ok(resp) => {
                let info = extract_block_info(&resp.block, &self.pool_addresses, &hash)?;
                Ok(Some(info))
            }
            Err(e) if is_block_not_found(&e) => {
                debug!(hash = %hash, "kaspad reports block-not-found");
                Ok(None)
            }
            Err(e) => Err(map_rpc_error_to_transport(&e)),
        }
    }
}

fn is_block_not_found(err: &RpcError) -> bool {
    // kaspad's RpcError taxonomy doesn't expose a "block not
    // found" variant cleanly across all transports, so we match
    // on the rendered error message. Each phrase below is one we
    // have observed in the wild:
    //   - "cannot find header"  — kaspad v1.1.0 / Toccata kaspad-tn10
    //     against an unknown hash (confirmed in the M3c dry-run
    //     against the operator's testnet-10 node).
    //   - "block not found"     — older kaspad versions.
    //   - "not in store"        — store-layer wording.
    //   - "missing block"       — selected-chain wording.
    // If a future kaspad version rewords this, the M3c live
    // exercise (`scripts/testnet10-tracker-live.sh`) catches the
    // drift before it reaches production by surfacing what would
    // otherwise be a silent "every submitted_to_node block looks
    // broken" symptom.
    let s = format!("{err}");
    let lc = s.to_lowercase();
    lc.contains("block not found")
        || lc.contains("not in store")
        || lc.contains("missing block")
        || lc.contains("cannot find header")
        || lc.contains("cannot find block")
}

fn map_rpc_error_to_transport(err: &RpcError) -> KaspadError {
    KaspadError::Transport(format!("{err}"))
}

/// Pure function: turn an `RpcBlock` + configured pool addresses
/// into the abstract `BlockInfo`. Factored out so it's
/// independently unit-testable against canned fixtures.
pub fn extract_block_info(
    block: &RpcBlock,
    pool_addresses: &[Address],
    expected_hash: &BlockHash,
) -> Result<BlockInfo, KaspadError> {
    let verbose = block.verbose_data.as_ref().ok_or_else(|| {
        KaspadError::Malformed(
            "block.verbose_data missing — kaspad gRPC server didn't populate verbose data"
                .to_owned(),
        )
    })?;

    // Sanity-check that the returned block is the one we asked
    // about. Kaspad shouldn't violate this but if it does we'd
    // rather surface the mismatch loudly than silently allocate
    // against the wrong block.
    if verbose.hash.as_bytes() != *expected_hash.as_bytes() {
        return Err(KaspadError::Malformed(format!(
            "kaspad returned block {} for request {}",
            verbose.hash, expected_hash
        )));
    }

    let coinbase_reward_sompi = extract_coinbase_reward(block, pool_addresses)?;
    Ok(BlockInfo {
        hash: *expected_hash,
        blue_score: verbose.blue_score,
        is_blue: verbose.is_chain_block,
        coinbase_reward_sompi,
        daa_score: block.header.daa_score,
    })
}

#[allow(clippy::cast_possible_wrap)]
fn extract_coinbase_reward(
    block: &RpcBlock,
    pool_addresses: &[Address],
) -> Result<i64, KaspadError> {
    // Coinbase tx is always transaction index 0 in a Kaspa block.
    let Some(coinbase) = block.transactions.first() else {
        return Err(KaspadError::Malformed(
            "block has zero transactions (no coinbase)".to_owned(),
        ));
    };
    let mut total_u64: u64 = 0;
    for (i, out) in coinbase.outputs.iter().enumerate() {
        let Some(vd) = out.verbose_data.as_ref() else {
            warn!(
                output_index = i,
                "coinbase output missing verbose_data; can't determine recipient address"
            );
            continue;
        };
        if pool_addresses.contains(&vd.script_public_key_address) {
            total_u64 = total_u64.checked_add(out.value).ok_or_else(|| {
                KaspadError::Malformed("coinbase reward sum overflows u64".to_owned())
            })?;
        }
    }
    // i64::MAX exceeds Kaspa's total supply in sompi, but be
    // defensive: a malformed/adversarial response could in
    // principle exceed it. Detect and surface.
    if total_u64 > i64::MAX as u64 {
        return Err(KaspadError::Malformed(
            "coinbase reward sum exceeds i64 range".to_owned(),
        ));
    }
    Ok(total_u64 as i64)
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::unwrap_used,
        clippy::panic,
        clippy::indexing_slicing,
        // Test-local integer arithmetic on safe ranges.
        clippy::integer_division
    )]
    use std::str::FromStr;

    use kaspa_addresses::{Prefix, Version};
    use kaspa_hashes::Hash as KaspaHash;
    use kaspa_rpc_core::RpcScriptClass;
    use kaspa_rpc_core::model::block::{RpcBlock, RpcBlockVerboseData};
    use kaspa_rpc_core::model::header::RpcHeader;
    use kaspa_rpc_core::model::tx::{
        RpcScriptPublicKey, RpcTransaction, RpcTransactionOutput, RpcTransactionOutputVerboseData,
    };

    use super::*;

    fn sample_address() -> Address {
        Address::try_from("kaspa:qypczcz0lhyf3tfsuqj86e7qc8us7r8a53nhlr4u6x4kq38td0hsjycf7sya7zq")
            .expect("parses")
    }

    fn sample_other_address() -> Address {
        Address::try_from("kaspa:qzncghl8re9h35hp6n5wyxtslhevj6462qkrkqzlfkrs2mpkfkc5xe9s3tga7")
            .expect("parses")
    }

    fn sample_hash() -> KaspaHash {
        KaspaHash::from_str("cc2b1da2c931f4164c03b2066cfb3178303567a161e8a393def62c91e824138a")
            .expect("hex")
    }

    fn sample_domain_hash() -> BlockHash {
        BlockHash::from_hex("cc2b1da2c931f4164c03b2066cfb3178303567a161e8a393def62c91e824138a")
            .expect("hex")
    }

    fn make_output(value: u64, addr: Option<Address>) -> RpcTransactionOutput {
        RpcTransactionOutput {
            value,
            script_public_key: RpcScriptPublicKey::new(Version::PubKey as u16, vec![].into()),
            verbose_data: addr.map(|a| RpcTransactionOutputVerboseData {
                script_public_key_type: RpcScriptClass::PubKey,
                script_public_key_address: a,
            }),
        }
    }

    fn make_coinbase(outputs: Vec<RpcTransactionOutput>) -> RpcTransaction {
        RpcTransaction {
            version: 0,
            inputs: vec![],
            outputs,
            lock_time: 0,
            subnetwork_id: kaspa_rpc_core::RpcSubnetworkId::from_bytes([0u8; 20]),
            gas: 0,
            payload: vec![],
            mass: 0,
            verbose_data: None,
        }
    }

    fn make_rpc_block(
        is_chain_block: bool,
        blue_score: u64,
        daa_score: u64,
        coinbase: RpcTransaction,
        hash: KaspaHash,
    ) -> RpcBlock {
        let header = RpcHeader {
            hash,
            version: 1,
            parents_by_level: vec![vec![]],
            hash_merkle_root: KaspaHash::default(),
            accepted_id_merkle_root: KaspaHash::default(),
            utxo_commitment: KaspaHash::default(),
            timestamp: 0,
            bits: 0,
            nonce: 0,
            daa_score,
            blue_work: 0u64.into(),
            blue_score,
            pruning_point: KaspaHash::default(),
        };
        let verbose = RpcBlockVerboseData {
            hash,
            difficulty: 1.0,
            selected_parent_hash: KaspaHash::default(),
            transaction_ids: vec![],
            is_header_only: false,
            blue_score,
            children_hashes: vec![],
            merge_set_blues_hashes: vec![],
            merge_set_reds_hashes: vec![],
            is_chain_block,
        };
        RpcBlock {
            header,
            transactions: vec![coinbase],
            verbose_data: Some(verbose),
        }
    }

    #[test]
    fn extract_sums_only_pool_outputs() {
        let pool = sample_address();
        let other = sample_other_address();
        let coinbase = make_coinbase(vec![
            make_output(50_000_000, Some(pool.clone())),
            make_output(70_000_000, Some(other)),
            make_output(30_000_000, Some(pool.clone())),
        ]);
        let block = make_rpc_block(true, 100, 1_000_000, coinbase, sample_hash());

        let info = extract_block_info(&block, &[pool], &sample_domain_hash()).unwrap();
        assert_eq!(info.coinbase_reward_sompi, 80_000_000);
        assert_eq!(info.blue_score, 100);
        assert_eq!(info.daa_score, 1_000_000);
        assert!(info.is_blue);
    }

    #[test]
    fn extract_returns_zero_when_no_outputs_match_pool_address() {
        let other = sample_other_address();
        let coinbase = make_coinbase(vec![make_output(100_000_000, Some(other))]);
        let block = make_rpc_block(true, 50, 500_000, coinbase, sample_hash());
        let info = extract_block_info(&block, &[sample_address()], &sample_domain_hash()).unwrap();
        assert_eq!(info.coinbase_reward_sompi, 0);
    }

    #[test]
    fn extract_supports_multiple_pool_addresses() {
        let a = sample_address();
        let b = sample_other_address();
        let coinbase = make_coinbase(vec![
            make_output(40_000_000, Some(a.clone())),
            make_output(60_000_000, Some(b.clone())),
        ]);
        let block = make_rpc_block(true, 200, 2_000_000, coinbase, sample_hash());
        let info = extract_block_info(&block, &[a, b], &sample_domain_hash()).unwrap();
        assert_eq!(info.coinbase_reward_sompi, 100_000_000);
    }

    #[test]
    fn extract_propagates_is_blue_false() {
        let pool = sample_address();
        let coinbase = make_coinbase(vec![make_output(100, Some(pool.clone()))]);
        let block = make_rpc_block(false, 0, 1, coinbase, sample_hash());
        let info = extract_block_info(&block, &[pool], &sample_domain_hash()).unwrap();
        assert!(!info.is_blue);
    }

    #[test]
    fn extract_rejects_block_with_missing_verbose_data() {
        let pool = sample_address();
        let coinbase = make_coinbase(vec![make_output(100, Some(pool.clone()))]);
        let mut block = make_rpc_block(true, 0, 1, coinbase, sample_hash());
        block.verbose_data = None;
        let err = extract_block_info(&block, &[pool], &sample_domain_hash()).unwrap_err();
        assert!(matches!(err, KaspadError::Malformed(_)));
    }

    #[test]
    fn extract_rejects_hash_mismatch() {
        let pool = sample_address();
        let coinbase = make_coinbase(vec![make_output(100, Some(pool.clone()))]);
        let block = make_rpc_block(true, 0, 1, coinbase, sample_hash());
        // Ask about a DIFFERENT hash than the block carries.
        let wrong =
            BlockHash::from_hex("9685f4347b9aa2e100bf489f7979a30746d90823d5bfb62309513b1e23ab2274")
                .unwrap();
        let err = extract_block_info(&block, &[pool], &wrong).unwrap_err();
        assert!(matches!(err, KaspadError::Malformed(_)));
    }

    #[test]
    fn extract_rejects_block_with_zero_transactions() {
        let pool = sample_address();
        let mut block = make_rpc_block(true, 0, 1, make_coinbase(vec![]), sample_hash());
        block.transactions.clear();
        let err = extract_block_info(&block, &[pool], &sample_domain_hash()).unwrap_err();
        assert!(matches!(err, KaspadError::Malformed(_)));
    }

    #[test]
    fn extract_skips_outputs_without_verbose_data() {
        let pool = sample_address();
        let coinbase = make_coinbase(vec![
            make_output(50_000_000, Some(pool.clone())),
            make_output(70_000_000, None), // no verbose_data → recipient unknown
        ]);
        let block = make_rpc_block(true, 1, 1, coinbase, sample_hash());
        let info = extract_block_info(&block, &[pool], &sample_domain_hash()).unwrap();
        // Only the recognised output counts.
        assert_eq!(info.coinbase_reward_sompi, 50_000_000);
    }

    #[test]
    fn extract_detects_overflow_in_sum() {
        let pool = sample_address();
        let coinbase = make_coinbase(vec![
            make_output(u64::MAX, Some(pool.clone())),
            make_output(1, Some(pool.clone())),
        ]);
        let block = make_rpc_block(true, 1, 1, coinbase, sample_hash());
        let err = extract_block_info(&block, &[pool], &sample_domain_hash()).unwrap_err();
        assert!(matches!(err, KaspadError::Malformed(m) if m.contains("overflow")));
    }

    #[test]
    fn extract_rejects_when_sum_exceeds_i64_max() {
        let pool = sample_address();
        // i64::MAX + 1 split across two outputs
        let half = u64::try_from(i64::MAX).unwrap() / 2 + 1;
        let coinbase = make_coinbase(vec![
            make_output(half, Some(pool.clone())),
            make_output(half, Some(pool.clone())),
        ]);
        let block = make_rpc_block(true, 1, 1, coinbase, sample_hash());
        let err = extract_block_info(&block, &[pool], &sample_domain_hash()).unwrap_err();
        assert!(matches!(err, KaspadError::Malformed(m) if m.contains("i64")));
    }

    // Prefix isn't actually used by extract_block_info; the
    // round-trip is only to confirm our sample_address constructor
    // produces a value with the prefix we expect from real testnet
    // payloads — guards against test-data drift.
    #[test]
    fn sample_address_uses_mainnet_prefix() {
        let a = sample_address();
        assert_eq!(a.prefix, Prefix::Mainnet);
    }
}
