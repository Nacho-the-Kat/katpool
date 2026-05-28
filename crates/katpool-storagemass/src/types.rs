//! Planner input/output types (consensus-native, pre-signing).

use kaspa_consensus_core::tx::{ScriptPublicKey, TransactionOutpoint, UtxoEntry};

use crate::evaluator::TxMass;

/// Spendable treasury UTXO available for payout funding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreasuryUtxo {
    /// Outpoint identifying the coin on chain.
    pub outpoint: TransactionOutpoint,
    /// Amount and script (must match chain state).
    pub entry: UtxoEntry,
}

/// One miner payout destination for a cycle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PayoutRecipient {
    /// Stable label for logs / deferred reports (wallet address, id, etc.).
    pub id: String,
    /// Payout amount in sompi (must be > 0).
    pub amount_sompi: u64,
    /// Recipient output script.
    pub script_public_key: ScriptPublicKey,
}

/// A mass-valid batch of inputs → payout outputs (+ optional change).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedBatch {
    /// Treasury inputs consumed by this transaction.
    pub inputs: Vec<TreasuryUtxo>,
    /// Recipients paid in this transaction.
    pub payouts: Vec<PayoutRecipient>,
    /// Change returned to the treasury (0 when exact spend).
    pub change_amount_sompi: u64,
    /// Masses computed for the planned shape (pre-signature).
    pub mass: TxMass,
}

/// Outcome of [`crate::plan_batches`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanBatchesResult {
    /// Mass-valid transactions to sign and broadcast, in planning order.
    pub batches: Vec<PlannedBatch>,
    /// Recipients below [`crate::MIN_PAYOUT_OUTPUT_SOMPI`] (held for next cycle).
    pub deferred_below_floor: Vec<PayoutRecipient>,
    /// Recipients that could not be funded from the supplied UTXO set.
    pub unpaid: Vec<PayoutRecipient>,
}
