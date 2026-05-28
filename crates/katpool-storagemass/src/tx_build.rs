//! Build unsigned `Transaction` + populated entries for mass evaluation.

use kaspa_consensus_core::{
    subnets::SubnetworkId,
    tx::{ScriptPublicKey, Transaction, TransactionInput, TransactionOutput, UtxoEntry},
};

use crate::types::{PayoutRecipient, TreasuryUtxo};

/// Standard subnetwork id used by the consensus mass tests (non-coinbase).
const fn standard_subnetwork() -> SubnetworkId {
    SubnetworkId::from_bytes([1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0])
}

/// Construct a populated transaction for mass checking (unsigned).
#[must_use]
pub fn build_populated(
    inputs: &[TreasuryUtxo],
    payouts: &[&PayoutRecipient],
    change_script: &ScriptPublicKey,
    change_amount_sompi: u64,
) -> (Transaction, Vec<UtxoEntry>) {
    let tx_inputs: Vec<TransactionInput> = inputs
        .iter()
        .map(|u| TransactionInput {
            previous_outpoint: u.outpoint,
            signature_script: vec![],
            sequence: 0,
            sig_op_count: 0,
        })
        .collect();

    let mut outputs: Vec<TransactionOutput> = payouts
        .iter()
        .map(|p| TransactionOutput {
            value: p.amount_sompi,
            script_public_key: p.script_public_key.clone(),
        })
        .collect();

    if change_amount_sompi > 0 {
        outputs.push(TransactionOutput {
            value: change_amount_sompi,
            script_public_key: change_script.clone(),
        });
    }

    let tx = Transaction::new(0, tx_inputs, outputs, 0, standard_subnetwork(), 0, vec![]);

    let entries: Vec<UtxoEntry> = inputs.iter().map(|u| u.entry.clone()).collect();
    (tx, entries)
}
