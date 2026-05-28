//! Planner tests: deferred floor, greedy packing, mass invariants.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::cast_possible_truncation
)]

use std::str::FromStr;

use kaspa_consensus_core::tx::{
    PopulatedTransaction, ScriptPublicKey, ScriptVec, TransactionId, TransactionOutpoint, UtxoEntry,
};
use katpool_storagemass::{
    MIN_PAYOUT_OUTPUT_SOMPI, MassEvaluator, PayoutRecipient, TreasuryUtxo, plan_batches,
};
use proptest::prelude::*;

fn empty_script() -> ScriptPublicKey {
    ScriptPublicKey::new(0, ScriptVec::from_slice(&[]))
}

fn sample_outpoint(index: u32) -> TransactionOutpoint {
    let tx_id =
        TransactionId::from_str("880eb9819a31821d9d2399e2f35e2433b72637e393d71ecc9b8d0250f49153c3")
            .unwrap();
    TransactionOutpoint {
        transaction_id: tx_id,
        index,
    }
}

fn treasury_utxo(index: u32, amount: u64) -> TreasuryUtxo {
    TreasuryUtxo {
        outpoint: sample_outpoint(index),
        entry: UtxoEntry {
            amount,
            script_public_key: empty_script(),
            block_daa_score: 0,
            is_coinbase: false,
        },
    }
}

fn recipient(id: &str, amount: u64) -> PayoutRecipient {
    PayoutRecipient {
        id: id.to_string(),
        amount_sompi: amount,
        script_public_key: empty_script(),
    }
}

fn assert_all_batches_mass_valid(
    evaluator: &MassEvaluator,
    result: &katpool_storagemass::PlanBatchesResult,
) {
    use katpool_storagemass::build_populated;
    for batch in &result.batches {
        let payout_refs: Vec<&PayoutRecipient> = batch.payouts.iter().collect();
        let (tx, entries) = build_populated(
            &batch.inputs,
            &payout_refs,
            &empty_script(),
            batch.change_amount_sompi,
        );
        let populated = PopulatedTransaction::new(&tx, entries);
        let mass = evaluator.evaluate_populated(&populated).unwrap();
        assert_eq!(mass, batch.mass);
        assert!(mass.fits_independently(evaluator.block_mass_limit()));
        let input_sum: u64 = batch.inputs.iter().map(|u| u.entry.amount).sum();
        let payout_sum: u64 = batch.payouts.iter().map(|p| p.amount_sompi).sum();
        assert_eq!(input_sum, payout_sum + batch.change_amount_sompi);
    }
}

#[test]
fn defers_recipients_below_output_floor() {
    let evaluator = MassEvaluator::mainnet();
    let change = empty_script();
    let utxos = vec![treasury_utxo(0, 10_000_000_000)];
    let recipients = vec![
        recipient("ok", 5_000_000_000),
        recipient("tiny", MIN_PAYOUT_OUTPUT_SOMPI - 1),
    ];

    let result = plan_batches(&evaluator, utxos, recipients, &change);
    assert_eq!(result.deferred_below_floor.len(), 1);
    assert_eq!(result.deferred_below_floor[0].id, "tiny");
    assert_eq!(result.batches.len(), 1);
    assert_eq!(result.batches[0].payouts.len(), 1);
    assert!(result.unpaid.is_empty());
    assert_all_batches_mass_valid(&evaluator, &result);
}

#[test]
fn packs_single_batch_when_mass_allows() {
    let evaluator = MassEvaluator::mainnet();
    let change = empty_script();
    let utxos = vec![treasury_utxo(0, 1_000_000_000_000)];
    let recipients = vec![
        recipient("a", 500_000_000_000),
        recipient("b", 400_000_000_000),
    ];

    let result = plan_batches(&evaluator, utxos, recipients, &change);
    assert_eq!(result.batches.len(), 1);
    assert_eq!(result.batches[0].payouts.len(), 2);
    assert_eq!(result.batches[0].change_amount_sompi, 100_000_000_000);
    assert!(result.unpaid.is_empty());
    assert_all_batches_mass_valid(&evaluator, &result);
}

#[test]
fn monolithic_fanout_exceeds_block_mass() {
    let evaluator = MassEvaluator::mainnet();
    let change = empty_script();
    let utxos = vec![treasury_utxo(0, 500_000_000_000_000)];
    let recipients: Vec<_> = (0..80)
        .map(|i| {
            recipient(
                &format!("dust{i}"),
                MIN_PAYOUT_OUTPUT_SOMPI + (i % 10) as u64,
            )
        })
        .collect();

    let payout_refs: Vec<&PayoutRecipient> = recipients.iter().collect();
    let (tx, entries) = katpool_storagemass::build_populated(
        &utxos,
        &payout_refs,
        &change,
        utxos[0].entry.amount - recipients.iter().map(|r| r.amount_sompi).sum::<u64>(),
    );
    let populated = PopulatedTransaction::new(&tx, entries);
    let mass = evaluator.evaluate_populated(&populated).unwrap();
    assert!(
        !mass.fits_independently(evaluator.block_mass_limit()),
        "fixture must exceed block mass when batched naively"
    );
}

#[test]
fn multiple_treasury_utxos_yield_multiple_batches() {
    let evaluator = MassEvaluator::mainnet();
    let change = empty_script();
    let utxos: Vec<_> = (0..6).map(|i| treasury_utxo(i, 100_000_000_000)).collect();
    let recipients: Vec<_> = (0..30)
        .map(|i| recipient(&format!("m{i}"), 5_000_000_000))
        .collect();

    let result = plan_batches(&evaluator, utxos, recipients, &change);
    assert!(
        result.batches.len() > 1,
        "expected multiple txs when funding set has several UTXOs"
    );
    let paid: usize = result.batches.iter().map(|b| b.payouts.len()).sum();
    assert_eq!(paid, 30);
    assert!(result.unpaid.is_empty());
    assert_all_batches_mass_valid(&evaluator, &result);
}

#[test]
fn leaves_unpaid_when_treasury_insufficient() {
    let evaluator = MassEvaluator::mainnet();
    let change = empty_script();
    let utxos = vec![treasury_utxo(0, 10_000_000_000)];
    let recipients = vec![recipient("a", 8_000_000_000), recipient("b", 8_000_000_000)];

    let result = plan_batches(&evaluator, utxos, recipients, &change);
    assert_eq!(result.batches.len(), 1);
    assert_eq!(result.batches[0].payouts.len(), 1);
    assert_eq!(result.unpaid.len(), 1);
    assert_all_batches_mass_valid(&evaluator, &result);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    #[test]
    fn every_planned_batch_fits_block_mass(
        input_amounts in prop::collection::vec(10_000_000u64..500_000_000_000, 1..4),
        payout_amounts in prop::collection::vec(MIN_PAYOUT_OUTPUT_SOMPI..50_000_000_000, 1..12),
    ) {
        let evaluator = MassEvaluator::mainnet();
        let change = empty_script();
        let utxos: Vec<_> = input_amounts
            .iter()
            .enumerate()
            .map(|(i, &amt)| treasury_utxo(i as u32, amt))
            .collect();
        let recipients: Vec<_> = payout_amounts
            .iter()
            .enumerate()
            .map(|(i, &amt)| recipient(&format!("w{i}"), amt))
            .collect();

        let result = plan_batches(&evaluator, utxos, recipients, &change);
        assert_all_batches_mass_valid(&evaluator, &result);

        for batch in &result.batches {
            for payout in &batch.payouts {
                prop_assert!(payout.amount_sompi >= MIN_PAYOUT_OUTPUT_SOMPI);
            }
        }
    }
}
