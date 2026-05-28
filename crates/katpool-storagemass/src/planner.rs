//! Greedy, mass-aware payout batch planner.

use kaspa_consensus_core::tx::ScriptPublicKey;

use crate::evaluator::{MassEvaluator, TxMass};
use kaspa_consensus_core::tx::PopulatedTransaction;

use crate::tx_build::build_populated;
use crate::types::{PayoutRecipient, PlanBatchesResult, PlannedBatch, TreasuryUtxo};

/// Heuristic from `docs/kips.md` §5.1: keep output count modest per input.
const MAX_OUTPUTS_PER_INPUT: usize = 10;

/// Partition recipients, sort funding set, and greedily pack mass-valid batches.
#[must_use]
pub fn plan_batches(
    evaluator: &MassEvaluator,
    mut utxos: Vec<TreasuryUtxo>,
    recipients: Vec<PayoutRecipient>,
    change_script: &ScriptPublicKey,
) -> PlanBatchesResult {
    let (mut payable, deferred_below_floor) = partition_by_floor(recipients);
    sort_utxos_desc(&mut utxos);
    sort_recipients_desc(&mut payable);

    let mut batches = Vec::new();
    while !payable.is_empty() && !utxos.is_empty() {
        let Some(batch) = build_one_batch(evaluator, &utxos, &payable, change_script) else {
            break;
        };
        remove_consumed(&mut utxos, &batch.inputs);
        remove_paid(&mut payable, &batch.payouts);
        batches.push(batch);
    }

    PlanBatchesResult {
        batches,
        deferred_below_floor,
        unpaid: payable,
    }
}

fn partition_by_floor(
    recipients: Vec<PayoutRecipient>,
) -> (Vec<PayoutRecipient>, Vec<PayoutRecipient>) {
    let floor = crate::MIN_PAYOUT_OUTPUT_SOMPI;
    let mut payable = Vec::new();
    let mut deferred = Vec::new();
    for rec in recipients {
        if rec.amount_sompi >= floor {
            payable.push(rec);
        } else {
            deferred.push(rec);
        }
    }
    (payable, deferred)
}

fn sort_utxos_desc(utxos: &mut [TreasuryUtxo]) {
    utxos.sort_by(|a, b| b.entry.amount.cmp(&a.entry.amount));
}

fn sort_recipients_desc(recipients: &mut [PayoutRecipient]) {
    recipients.sort_by(|a, b| b.amount_sompi.cmp(&a.amount_sompi));
}

fn build_one_batch(
    evaluator: &MassEvaluator,
    utxos: &[TreasuryUtxo],
    recipients: &[PayoutRecipient],
    change_script: &ScriptPublicKey,
) -> Option<PlannedBatch> {
    let max_inputs = utxos.len();
    let mut input_count = 1;

    while input_count <= max_inputs {
        let inputs = utxos.get(..input_count)?;
        let input_sum: u64 = inputs.iter().map(|u| u.entry.amount).sum();

        let selected =
            greedy_recipients_for_inputs(evaluator, inputs, input_sum, recipients, change_script)?;

        if selected.is_empty() {
            input_count += 1;
            continue;
        }

        let payout_sum: u64 = selected
            .iter()
            .filter_map(|&idx| recipients.get(idx).map(|r| r.amount_sompi))
            .sum();
        if payout_sum > input_sum {
            input_count += 1;
            continue;
        }

        let change_amount = input_sum - payout_sum;
        let payout_refs: Vec<&PayoutRecipient> = selected
            .iter()
            .filter_map(|&idx| recipients.get(idx))
            .collect();
        if payout_refs.len() != selected.len() {
            input_count += 1;
            continue;
        }

        if let Some(mass) = evaluate_shape(
            evaluator,
            inputs,
            &payout_refs,
            change_script,
            change_amount,
        ) {
            let payouts: Vec<PayoutRecipient> = selected
                .iter()
                .filter_map(|&idx| recipients.get(idx).cloned())
                .collect();
            return Some(PlannedBatch {
                inputs: inputs.to_vec(),
                payouts,
                change_amount_sompi: change_amount,
                mass,
            });
        }

        input_count += 1;
    }

    None
}

/// Greedily grow a recipient set (largest-first order) while mass fits.
fn greedy_recipients_for_inputs(
    evaluator: &MassEvaluator,
    inputs: &[TreasuryUtxo],
    input_sum: u64,
    recipients: &[PayoutRecipient],
    change_script: &ScriptPublicKey,
) -> Option<Vec<usize>> {
    let max_outputs = inputs.len().saturating_mul(MAX_OUTPUTS_PER_INPUT).max(1);

    let mut selected: Vec<usize> = Vec::new();
    for (idx, rec) in recipients.iter().enumerate() {
        if selected.len() >= max_outputs {
            break;
        }
        let candidate_sum: u64 = selected
            .iter()
            .filter_map(|&i| recipients.get(i).map(|r| r.amount_sompi))
            .sum::<u64>()
            .saturating_add(rec.amount_sompi);
        if candidate_sum > input_sum {
            continue;
        }
        let mut candidate = selected.clone();
        candidate.push(idx);
        let payout_refs: Vec<&PayoutRecipient> = candidate
            .iter()
            .filter_map(|&i| recipients.get(i))
            .collect();
        if payout_refs.len() != candidate.len() {
            continue;
        }
        let change = input_sum - candidate_sum;
        if evaluate_shape(evaluator, inputs, &payout_refs, change_script, change).is_some() {
            selected = candidate;
        }
    }

    if selected.is_empty() {
        None
    } else {
        Some(selected)
    }
}

fn evaluate_shape(
    evaluator: &MassEvaluator,
    inputs: &[TreasuryUtxo],
    payouts: &[&PayoutRecipient],
    change_script: &ScriptPublicKey,
    change_amount: u64,
) -> Option<TxMass> {
    let (tx, entries) = build_populated(inputs, payouts, change_script, change_amount);
    let populated = PopulatedTransaction::new(&tx, entries);
    let mass = evaluator.evaluate_populated(&populated).ok()?;
    mass.fits_independently(evaluator.block_mass_limit())
        .then_some(mass)
}

fn remove_consumed(utxos: &mut Vec<TreasuryUtxo>, consumed: &[TreasuryUtxo]) {
    utxos.retain(|u| {
        !consumed
            .iter()
            .any(|c| c.outpoint == u.outpoint && c.entry.amount == u.entry.amount)
    });
}

fn remove_paid(recipients: &mut Vec<PayoutRecipient>, paid: &[PayoutRecipient]) {
    recipients.retain(|r| !paid.iter().any(|p| p.id == r.id));
}
