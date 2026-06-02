//! Real [`KaspadClient`] implementation backed by `kaspa-grpc-client`.
//!
//! Translates the tracker's three-method surface into kaspad gRPC
//! calls:
//!
//! - [`KaspadClient::get_virtual_daa_score`] → [`RpcApi::get_block_dag_info`]
//!   (`virtual_daa_score`).
//! - [`KaspadClient::get_block_color`] → [`RpcApi::get_current_block_color`].
//!   `Ok` carries the GHOSTDAG colour; `RpcError::MergerNotFound`
//!   (the block is not yet merged into the sink's past, or kaspad has
//!   not seen it) maps to [`BlockColor::NotYetMerged`].
//! - [`KaspadClient::get_pool_coinbase_utxos`] → [`RpcApi::get_utxos_by_addresses`]
//!   for the configured pool address(es), keeping only coinbase UTXOs.
//!
//! ## Why these calls
//!
//! See ADR-0014: block "blueness" is GHOSTDAG colour
//! (`get_current_block_color`), not selected-parent-chain membership;
//! and the pool's realised reward is the coinbase UTXOs credited to the
//! pool address (`get_utxos_by_addresses`), not the found block's own
//! coinbase. `get_utxos_by_addresses` requires the node to run with
//! `--utxoindex` (else `RpcError::NoUtxoIndex`, surfaced as a transport
//! error here so the operator sees it loudly).

use std::sync::Arc;

use async_trait::async_trait;
use kaspa_addresses::Address;
use kaspa_grpc_client::GrpcClient;
use kaspa_hashes::Hash as KaspaHash;
use kaspa_rpc_core::RpcError;
use kaspa_rpc_core::api::rpc::RpcApi;
use kaspa_rpc_core::model::address::RpcUtxosByAddressesEntry;
use kaspa_rpc_core::model::message::GetCurrentBlockColorRequest;

use katpool_domain::BlockHash;
use tracing::debug;

use crate::maturity::{BlockColor, CoinbaseUtxo, KaspadClient, KaspadError};

/// Backing client for the real-kaspad implementation.
pub struct KaspadGrpcClient {
    client: Arc<GrpcClient>,
    pool_addresses: Vec<Address>,
}

impl KaspadGrpcClient {
    /// Construct from an already-connected `GrpcClient`. The caller
    /// owns the connection lifecycle (reconnect, timeouts).
    ///
    /// `pool_addresses` is the address set whose coinbase UTXOs count
    /// as pool revenue. Pass exactly one for the common case; multiple
    /// is supported for future hot/cold setups.
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
    async fn get_virtual_daa_score(&self) -> Result<u64, KaspadError> {
        self.client
            .get_block_dag_info()
            .await
            .map(|info| info.virtual_daa_score)
            .map_err(|e| map_rpc_error_to_transport(&e))
    }

    async fn get_block_color(&self, hash: BlockHash) -> Result<BlockColor, KaspadError> {
        let kaspa_hash = KaspaHash::from_bytes(*hash.as_bytes());
        let request = GetCurrentBlockColorRequest { hash: kaspa_hash };
        match self
            .client
            .get_current_block_color_call(None, request)
            .await
        {
            Ok(resp) => Ok(if resp.blue {
                BlockColor::Blue
            } else {
                BlockColor::Red
            }),
            Err(RpcError::MergerNotFound(_)) => {
                debug!(hash = %hash, "kaspad reports block not yet merged");
                Ok(BlockColor::NotYetMerged)
            }
            Err(e) => Err(map_rpc_error_to_transport(&e)),
        }
    }

    async fn get_pool_coinbase_utxos(&self) -> Result<Vec<CoinbaseUtxo>, KaspadError> {
        let entries = self
            .client
            .get_utxos_by_addresses(self.pool_addresses.clone())
            .await
            .map_err(|e| map_rpc_error_to_transport(&e))?;
        Ok(coinbase_utxos_from_entries(&entries))
    }
}

fn map_rpc_error_to_transport(err: &RpcError) -> KaspadError {
    KaspadError::Transport(format!("{err}"))
}

/// Pure function: keep only the coinbase UTXOs from a
/// `get_utxos_by_addresses` response and project them into the
/// tracker's [`CoinbaseUtxo`]. Factored out for unit testing.
#[must_use]
pub fn coinbase_utxos_from_entries(entries: &[RpcUtxosByAddressesEntry]) -> Vec<CoinbaseUtxo> {
    entries
        .iter()
        .filter(|e| e.utxo_entry.is_coinbase)
        .map(|e| CoinbaseUtxo {
            transaction_id: e.outpoint.transaction_id.as_bytes(),
            index: e.outpoint.index,
            amount_sompi: e.utxo_entry.amount,
            block_daa_score: e.utxo_entry.block_daa_score,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::unwrap_used,
        clippy::panic,
        clippy::indexing_slicing,
        // `RpcUtxoEntry::script_public_key`'s type lives in a crate the
        // accountant doesn't depend on directly; `Default::default()`
        // is the only way to name it from here.
        clippy::default_trait_access
    )]
    use std::str::FromStr;

    use kaspa_hashes::Hash as KaspaHash;
    use kaspa_rpc_core::RpcUtxoEntry;
    use kaspa_rpc_core::model::address::RpcUtxosByAddressesEntry;
    use kaspa_rpc_core::model::tx::RpcTransactionOutpoint;

    use super::*;

    fn entry(
        amount: u64,
        daa: u64,
        index: u32,
        is_coinbase: bool,
        txid: &str,
    ) -> RpcUtxosByAddressesEntry {
        RpcUtxosByAddressesEntry {
            address: None,
            outpoint: RpcTransactionOutpoint {
                transaction_id: KaspaHash::from_str(txid).expect("hex"),
                index,
            },
            utxo_entry: RpcUtxoEntry {
                amount,
                script_public_key: Default::default(),
                block_daa_score: daa,
                is_coinbase,
                covenant_id: None,
            },
        }
    }

    const TXID_A: &str = "cc2b1da2c931f4164c03b2066cfb3178303567a161e8a393def62c91e824138a";
    const TXID_B: &str = "9685f4347b9aa2e100bf489f7979a30746d90823d5bfb62309513b1e23ab2274";

    #[test]
    fn keeps_only_coinbase_utxos() {
        let entries = vec![
            entry(500_000_000, 1_000, 0, true, TXID_A),
            entry(123, 1_001, 3, false, TXID_B),
        ];
        let out = coinbase_utxos_from_entries(&entries);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].amount_sompi, 500_000_000);
        assert_eq!(out[0].block_daa_score, 1_000);
        assert_eq!(out[0].index, 0);
        assert_eq!(
            out[0].transaction_id,
            KaspaHash::from_str(TXID_A).unwrap().as_bytes()
        );
    }

    #[test]
    fn empty_when_no_coinbase() {
        let entries = vec![entry(1, 1, 0, false, TXID_A)];
        assert!(coinbase_utxos_from_entries(&entries).is_empty());
    }
}
