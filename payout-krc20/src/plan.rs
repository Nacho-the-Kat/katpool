//! Mass-aware KRC-20 commit/reveal planner.
//!
//! A KRC-20 NACHO transfer is two transactions (see [ADR-0015],
//! `docs/kips.md` §5.2):
//!
//! - **commit** — funds a P2SH output whose redeem script is the kasplex
//!   inscription envelope, plus change back to the treasury;
//! - **reveal** — spends that single P2SH output (exposing the inscription
//!   in its signature script) and returns the funds, minus the reveal fee,
//!   to the treasury. Exactly **one input, one output** keeps it small.
//!
//! Both transactions must independently satisfy every KIP-9/KIP-13 mass
//! against `max_block_mass`. The reveal is the interesting one: its
//! `transient_storage_mass` is driven by the redeem-script-and-data path in
//! the signature script, so — unlike the KAS planner, which evaluates
//! unsigned shapes — this planner sizes the signature scripts to their
//! **signed** length before evaluating. The 32-byte Schnorr signature push
//! is [`STANDARD_SIGNATURE_SCRIPT_LEN`] bytes (rusty-kaspa
//! `wallet::tx::mass::SIGNATURE_SIZE`); the reveal additionally carries the
//! canonical push of the full redeem script.
//!
//! [ADR-0015]: ../../../docs/decisions/0015-krc20-inscription-envelope.md

use kaspa_consensus_core::{
    subnets::SUBNETWORK_ID_NATIVE,
    tx::{
        PopulatedTransaction, ScriptPublicKey, Transaction, TransactionId, TransactionInput,
        TransactionOutpoint, TransactionOutput, UtxoEntry,
    },
};
use katpool_storagemass::{
    MIN_PAYOUT_OUTPUT_SOMPI, MassEvaluationError, MassEvaluator, TreasuryUtxo, TxMass,
};

use crate::inscription::{
    InscriptionError, Krc20Transfer, build_transfer_inscription, commit_script_public_key,
    reveal_signature_script,
};

/// Signed length of a standard Schnorr P2PK signature script, in bytes:
/// `OP_DATA_65 (1) + 64-byte signature + 1 sighash byte`. Matches
/// rusty-kaspa `wallet::tx::mass::SIGNATURE_SIZE`.
pub const STANDARD_SIGNATURE_SCRIPT_LEN: usize = 66;

/// Default commit/reveal fee, mirroring the legacy pool's fixed
/// `0.0001 KAS` (`10_000` sompi).
pub const DEFAULT_FEE_SOMPI: u64 = 10_000;

/// Default amount locked into the commit P2SH output (`0.2 KAS`).
///
/// Spent in full by the reveal and returned to the treasury minus the
/// reveal fee, so it is a transient lock, not a cost. Must exceed
/// `reveal_fee + MIN_PAYOUT_OUTPUT_SOMPI`.
pub const DEFAULT_COMMIT_AMOUNT_SOMPI: u64 = 20_000_000;

/// Per-input sigop count: one `OP_CHECKSIG` per standard or P2SH input.
const SIG_OP_COUNT_PER_INPUT: u8 = 1;

/// Fees and amounts that size the commit/reveal pair (mass-irrelevant
/// beyond their effect on output values; collected here for clarity).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommitRevealConfig {
    /// Amount locked into the commit P2SH output.
    pub commit_amount_sompi: u64,
    /// Fee paid by the commit transaction.
    pub commit_fee_sompi: u64,
    /// Fee paid by the reveal transaction.
    pub reveal_fee_sompi: u64,
}

impl Default for CommitRevealConfig {
    fn default() -> Self {
        Self {
            commit_amount_sompi: DEFAULT_COMMIT_AMOUNT_SOMPI,
            commit_fee_sompi: DEFAULT_FEE_SOMPI,
            reveal_fee_sompi: DEFAULT_FEE_SOMPI,
        }
    }
}

/// A mass-valid KRC-20 commit/reveal pair for one NACHO transfer.
#[derive(Debug, Clone)]
pub struct PlannedCommitReveal {
    /// The inscription redeem script (commit P2SH preimage; reveal exposes it).
    pub redeem_script: Vec<u8>,
    /// The P2SH script public key the commit pays to.
    pub commit_script_public_key: ScriptPublicKey,
    /// Amount locked into the commit P2SH output (the reveal spends it in full).
    pub commit_amount_sompi: u64,
    /// Treasury inputs the commit consumes.
    pub commit_inputs: Vec<TreasuryUtxo>,
    /// Change returned to the treasury by the commit (0 when folded to fee).
    pub commit_change_sompi: u64,
    /// The commit transaction's three masses (signed-size accurate).
    pub commit_mass: TxMass,
    /// Amount the reveal returns to the treasury (`commit_amount − reveal_fee`).
    pub reveal_return_sompi: u64,
    /// The reveal transaction's three masses (includes the redeem-script push).
    pub reveal_mass: TxMass,
}

/// Reasons a commit/reveal pair cannot be planned.
#[derive(Debug, thiserror::Error)]
pub enum PlanError {
    /// The inscription envelope could not be built.
    #[error("inscription: {0}")]
    Inscription(#[from] InscriptionError),

    /// Treasury UTXOs cannot cover `commit_amount + commit_fee`.
    #[error("insufficient treasury funds: need {needed_sompi} sompi, have {available_sompi}")]
    InsufficientFunds {
        /// Sompi required (`commit_amount + commit_fee`).
        needed_sompi: u64,
        /// Sompi available across the supplied treasury UTXOs.
        available_sompi: u64,
    },

    /// `commit_amount − reveal_fee` is below the dust floor, so the reveal
    /// output would be unspendable / non-standard.
    #[error("reveal return {return_sompi} sompi below floor {floor_sompi}")]
    RevealBelowFloor {
        /// The computed reveal return.
        return_sompi: u64,
        /// [`MIN_PAYOUT_OUTPUT_SOMPI`].
        floor_sompi: u64,
    },

    /// The commit transaction exceeds the block mass limit on some component.
    #[error("commit mass exceeds block limit {limit}: {mass:?}")]
    CommitMassExceeded {
        /// The offending masses.
        mass: TxMass,
        /// The block mass limit.
        limit: u64,
    },

    /// The reveal transaction exceeds the block mass limit on some component.
    #[error("reveal mass exceeds block limit {limit}: {mass:?}")]
    RevealMassExceeded {
        /// The offending masses.
        mass: TxMass,
        /// The block mass limit.
        limit: u64,
    },

    /// Consensus could not compute mass for the planned shape.
    #[error("mass evaluation: {0}")]
    MassEval(#[from] MassEvaluationError),
}

/// Plans the commit/reveal pair for a single KRC-20 transfer and verifies
/// both transactions fit every mass independently.
///
/// `treasury_script` is the treasury's P2PK script (for commit change and
/// the reveal return). `xonly_pubkey` is the treasury's 32-byte Schnorr key
/// bound into the inscription.
///
/// # Errors
///
/// See [`PlanError`]: bad inscription, underfunded commit, sub-floor reveal
/// return, either transaction over the mass limit, or an incomputable mass.
pub fn plan_commit_reveal(
    evaluator: &MassEvaluator,
    treasury_utxos: &[TreasuryUtxo],
    treasury_script: &ScriptPublicKey,
    xonly_pubkey: &[u8; 32],
    transfer: &Krc20Transfer,
    cfg: &CommitRevealConfig,
) -> Result<PlannedCommitReveal, PlanError> {
    // ---- cheap config precondition: reveal return must clear the floor
    let reveal_return_sompi = cfg
        .commit_amount_sompi
        .checked_sub(cfg.reveal_fee_sompi)
        .filter(|r| *r >= MIN_PAYOUT_OUTPUT_SOMPI)
        .ok_or_else(|| PlanError::RevealBelowFloor {
            return_sompi: cfg.commit_amount_sompi.saturating_sub(cfg.reveal_fee_sompi),
            floor_sompi: MIN_PAYOUT_OUTPUT_SOMPI,
        })?;

    let redeem_script = build_transfer_inscription(xonly_pubkey, transfer)?;
    let commit_spk = commit_script_public_key(&redeem_script);

    // ---- fund + build the commit ------------------------------------
    let needed = cfg.commit_amount_sompi.saturating_add(cfg.commit_fee_sompi);
    let (commit_inputs, input_sum) = select_inputs(treasury_utxos, needed)?;

    // Leftover after the locked amount and fee is change; fold sub-floor
    // change into the fee rather than create a dust output. Note: a change
    // output that clears the dust floor can still fail KIP-9 storage mass if
    // it is small relative to a large funding input (anti-dust). That is
    // reported as `CommitMassExceeded`; UTXO hygiene to avoid it belongs to
    // the execute/maintain layers (docs/kips.md §5.3–§5.4), not the planner.
    let leftover = input_sum - needed;
    let commit_change_sompi = if leftover >= MIN_PAYOUT_OUTPUT_SOMPI {
        leftover
    } else {
        0
    };

    let mut commit_outputs = vec![TransactionOutput {
        value: cfg.commit_amount_sompi,
        script_public_key: commit_spk.clone(),
    }];
    if commit_change_sompi > 0 {
        commit_outputs.push(TransactionOutput {
            value: commit_change_sompi,
            script_public_key: treasury_script.clone(),
        });
    }
    let commit_tx = build_tx(
        commit_inputs
            .iter()
            .map(|u| (u.outpoint, vec![0u8; STANDARD_SIGNATURE_SCRIPT_LEN]))
            .collect(),
        commit_outputs,
    );
    let commit_entries: Vec<UtxoEntry> = commit_inputs.iter().map(|u| u.entry.clone()).collect();
    let commit_mass = evaluate(evaluator, &commit_tx, commit_entries)?;
    if !commit_mass.fits_independently(evaluator.block_mass_limit()) {
        return Err(PlanError::CommitMassExceeded {
            mass: commit_mass,
            limit: evaluator.block_mass_limit(),
        });
    }

    // ---- build the reveal (1 input from the commit, 1 output) -------
    // The commit txid is unknown until the commit is signed; the outpoint
    // value does not affect mass (fixed 32+4 bytes), so a placeholder
    // outpoint at the commit's P2SH output index suffices for planning.
    let commit_p2sh_outpoint = TransactionOutpoint {
        transaction_id: TransactionId::from_bytes([0u8; 32]),
        index: 0,
    };
    let reveal_sig_script = reveal_signature_script(
        redeem_script.clone(),
        vec![0u8; STANDARD_SIGNATURE_SCRIPT_LEN],
    )?;
    let reveal_tx = build_tx(
        vec![(commit_p2sh_outpoint, reveal_sig_script)],
        vec![TransactionOutput {
            value: reveal_return_sompi,
            script_public_key: treasury_script.clone(),
        }],
    );
    let reveal_entry = UtxoEntry {
        amount: cfg.commit_amount_sompi,
        script_public_key: commit_spk.clone(),
        block_daa_score: 0,
        is_coinbase: false,
    };
    let reveal_mass = evaluate(evaluator, &reveal_tx, vec![reveal_entry])?;
    if !reveal_mass.fits_independently(evaluator.block_mass_limit()) {
        return Err(PlanError::RevealMassExceeded {
            mass: reveal_mass,
            limit: evaluator.block_mass_limit(),
        });
    }

    Ok(PlannedCommitReveal {
        redeem_script,
        commit_script_public_key: commit_spk,
        commit_amount_sompi: cfg.commit_amount_sompi,
        commit_inputs,
        commit_change_sompi,
        commit_mass,
        reveal_return_sompi,
        reveal_mass,
    })
}

/// Greedily selects treasury UTXOs (largest first) until they cover `needed`.
fn select_inputs(
    treasury_utxos: &[TreasuryUtxo],
    needed: u64,
) -> Result<(Vec<TreasuryUtxo>, u64), PlanError> {
    let mut sorted: Vec<TreasuryUtxo> = treasury_utxos.to_vec();
    sorted.sort_by(|a, b| b.entry.amount.cmp(&a.entry.amount));

    let available: u64 = sorted.iter().map(|u| u.entry.amount).sum();
    let mut selected = Vec::new();
    let mut sum = 0u64;
    for utxo in sorted {
        if sum >= needed {
            break;
        }
        sum = sum.saturating_add(utxo.entry.amount);
        selected.push(utxo);
    }
    if sum < needed {
        return Err(PlanError::InsufficientFunds {
            needed_sompi: needed,
            available_sompi: available,
        });
    }
    Ok((selected, sum))
}

/// Builds an unsigned-shape transaction whose inputs already carry the
/// signed-length signature scripts (so transient mass is accurate).
fn build_tx(
    inputs: Vec<(TransactionOutpoint, Vec<u8>)>,
    outputs: Vec<TransactionOutput>,
) -> Transaction {
    let tx_inputs: Vec<TransactionInput> = inputs
        .into_iter()
        .map(|(previous_outpoint, signature_script)| TransactionInput {
            previous_outpoint,
            signature_script,
            sequence: 0,
            sig_op_count: SIG_OP_COUNT_PER_INPUT,
        })
        .collect();
    Transaction::new(0, tx_inputs, outputs, 0, SUBNETWORK_ID_NATIVE, 0, vec![])
}

/// Evaluates the three masses for `tx` populated with `entries`.
fn evaluate(
    evaluator: &MassEvaluator,
    tx: &Transaction,
    entries: Vec<UtxoEntry>,
) -> Result<TxMass, MassEvaluationError> {
    let populated = PopulatedTransaction::new(tx, entries);
    evaluator.evaluate_populated(&populated)
}
