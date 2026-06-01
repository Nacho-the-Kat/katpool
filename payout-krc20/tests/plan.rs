//! Deterministic, chain-free tests for the mass-aware commit/reveal planner.
//! The headline guarantee: the reveal's transient mass accounts for the
//! redeem-script push, and both transactions fit `max_block_mass`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use kaspa_addresses::{Address, Prefix, Version};
use kaspa_consensus_core::tx::{ScriptPublicKey, TransactionId};
use kaspa_consensus_core::tx::{TransactionOutpoint, UtxoEntry};
use kaspa_txscript::pay_to_address_script;
use katpool_storagemass::{MIN_PAYOUT_OUTPUT_SOMPI, MassEvaluator, TreasuryUtxo};
use payout_krc20::{CommitRevealConfig, Krc20Transfer, PlanError, plan_commit_reveal};

const XONLY_PK: [u8; 32] = [
    0x1b, 0x91, 0x5b, 0x4c, 0x2a, 0x77, 0x0e, 0x3f, 0x44, 0x09, 0xd8, 0x60, 0xb1, 0x22, 0x6e, 0x90,
    0xc5, 0x33, 0xaa, 0x18, 0x7d, 0x6f, 0x4e, 0x21, 0x88, 0x99, 0x10, 0x55, 0xe7, 0x3c, 0xba, 0x02,
];

const RECIPIENT: &str = "kaspatest:qqkq3vz9j8m8k0r2c8x4n5p6w7s9t0u1v2x3y4z5a6b7c8d9e0f";

fn treasury_script() -> ScriptPublicKey {
    let addr = Address::new(Prefix::Testnet, Version::PubKey, &XONLY_PK);
    pay_to_address_script(&addr)
}

fn utxo(index: u32, amount: u64) -> TreasuryUtxo {
    TreasuryUtxo {
        outpoint: TransactionOutpoint {
            transaction_id: TransactionId::from_bytes([7u8; 32]),
            index,
        },
        entry: UtxoEntry {
            amount,
            script_public_key: treasury_script(),
            block_daa_score: 0,
            is_coinbase: false,
            covenant_id: None,
        },
    }
}

fn transfer() -> Krc20Transfer {
    Krc20Transfer::new("NACHO", "273972602739", RECIPIENT)
}

#[test]
fn plans_pair_and_both_fit_independently() {
    let eval = MassEvaluator::mainnet();
    let cfg = CommitRevealConfig::default();
    let utxos = vec![utxo(0, 5_000_000_000)]; // 50 KAS

    let plan = plan_commit_reveal(
        &eval,
        &utxos,
        &treasury_script(),
        &XONLY_PK,
        &transfer(),
        &cfg,
    )
    .unwrap();

    assert!(plan.commit_mass.fits_independently(eval.block_mass_limit()));
    assert!(plan.reveal_mass.fits_independently(eval.block_mass_limit()));
    assert_eq!(
        plan.reveal_return_sompi,
        cfg.commit_amount_sompi - cfg.reveal_fee_sompi
    );
    assert!(!plan.commit_inputs.is_empty());
    assert!(!plan.redeem_script.is_empty());
}

#[test]
fn reveal_transient_mass_accounts_for_redeem_script() {
    let eval = MassEvaluator::mainnet();
    let cfg = CommitRevealConfig::default();
    let utxos = vec![utxo(0, 5_000_000_000)];

    let plan = plan_commit_reveal(
        &eval,
        &utxos,
        &treasury_script(),
        &XONLY_PK,
        &transfer(),
        &cfg,
    )
    .unwrap();

    // Transient mass = serialized_size × 4, and the serialized reveal
    // embeds the full redeem-script push in its signature script. So the
    // transient mass must exceed 4× the redeem-script length — proof that
    // KIP-13's redeem-script-and-data path is counted (docs/kips.md §5.2).
    let redeem_len = plan.redeem_script.len() as u64;
    assert!(
        plan.reveal_mass.transient_mass > redeem_len * 4,
        "reveal transient {} should exceed 4×redeem {}",
        plan.reveal_mass.transient_mass,
        redeem_len * 4
    );
}

#[test]
fn underfunded_treasury_is_rejected() {
    let eval = MassEvaluator::mainnet();
    let cfg = CommitRevealConfig::default();
    let utxos = vec![utxo(0, 1_000)]; // far below commit_amount + fee

    let err = plan_commit_reveal(
        &eval,
        &utxos,
        &treasury_script(),
        &XONLY_PK,
        &transfer(),
        &cfg,
    )
    .expect_err("should be underfunded");
    assert!(
        matches!(err, PlanError::InsufficientFunds { .. }),
        "got {err:?}"
    );
}

#[test]
fn reveal_return_below_floor_is_rejected() {
    let eval = MassEvaluator::mainnet();
    // commit_amount only just above the reveal fee → return < dust floor.
    let cfg = CommitRevealConfig {
        commit_amount_sompi: 10_000 + 5, // reveal_fee + 5
        commit_fee_sompi: 10_000,
        reveal_fee_sompi: 10_000,
    };
    let utxos = vec![utxo(0, 5_000_000_000)];

    let err = plan_commit_reveal(
        &eval,
        &utxos,
        &treasury_script(),
        &XONLY_PK,
        &transfer(),
        &cfg,
    )
    .expect_err("should reject sub-floor reveal");
    assert!(
        matches!(err, PlanError::RevealBelowFloor { .. }),
        "got {err:?}"
    );
}

#[test]
fn sub_floor_change_folds_into_fee() {
    let eval = MassEvaluator::mainnet();
    let cfg = CommitRevealConfig::default();
    // Exactly needed + (floor − 1): leftover is below the dust floor.
    let needed = cfg.commit_amount_sompi + cfg.commit_fee_sompi;
    let utxos = vec![utxo(0, needed + MIN_PAYOUT_OUTPUT_SOMPI - 1)];

    let plan = plan_commit_reveal(
        &eval,
        &utxos,
        &treasury_script(),
        &XONLY_PK,
        &transfer(),
        &cfg,
    )
    .unwrap();
    assert_eq!(
        plan.commit_change_sompi, 0,
        "sub-floor change must be folded into the fee"
    );
}

#[test]
fn healthy_change_is_returned() {
    let eval = MassEvaluator::mainnet();
    let cfg = CommitRevealConfig::default();
    let needed = cfg.commit_amount_sompi + cfg.commit_fee_sompi;
    // Leftover large enough to clear both the dust floor and KIP-9 storage
    // mass (small outputs from a large input are penalised — anti-dust).
    let leftover = 50_000_000; // 0.5 KAS
    let utxos = vec![utxo(0, needed + leftover)];

    let plan = plan_commit_reveal(
        &eval,
        &utxos,
        &treasury_script(),
        &XONLY_PK,
        &transfer(),
        &cfg,
    )
    .unwrap();
    assert_eq!(plan.commit_change_sompi, leftover);
    assert!(plan.commit_mass.fits_independently(eval.block_mass_limit()));
}

#[test]
fn near_floor_change_from_large_input_fails_storage_mass() {
    // KIP-9 anti-dust: a change output that clears the economic floor can
    // still blow storage mass when funded by a much larger input. The
    // planner must report this rather than emit an unminable transaction.
    let eval = MassEvaluator::mainnet();
    let cfg = CommitRevealConfig::default();
    let needed = cfg.commit_amount_sompi + cfg.commit_fee_sompi;
    let utxos = vec![utxo(0, needed + MIN_PAYOUT_OUTPUT_SOMPI)]; // leftover == floor

    let err = plan_commit_reveal(
        &eval,
        &utxos,
        &treasury_script(),
        &XONLY_PK,
        &transfer(),
        &cfg,
    )
    .expect_err("floor-sized change from a large input should fail storage mass");
    assert!(
        matches!(err, PlanError::CommitMassExceeded { .. }),
        "got {err:?}"
    );
}

#[test]
fn selects_multiple_inputs_when_needed() {
    let eval = MassEvaluator::mainnet();
    let cfg = CommitRevealConfig::default();
    let needed = cfg.commit_amount_sompi + cfg.commit_fee_sompi;
    // Two UTXOs, each individually below `needed` so both must be consumed,
    // but together comfortably covering it.
    let part = needed - MIN_PAYOUT_OUTPUT_SOMPI;
    let utxos = vec![utxo(0, part), utxo(1, part)];

    let plan = plan_commit_reveal(
        &eval,
        &utxos,
        &treasury_script(),
        &XONLY_PK,
        &transfer(),
        &cfg,
    )
    .unwrap();
    assert_eq!(
        plan.commit_inputs.len(),
        2,
        "should consume both UTXOs to cover the commit"
    );
}
