//! KRC-20 NACHO payout dry-run rehearsal — evidence assembly.
//!
//! The binary ([`main`](../katpool_krc20_rehearsal/index.html)) drives one
//! dry-run NACHO cycle through the production engine
//! ([`payout_krc20::Krc20PayoutEngine`] with
//! [`payout_kas::ExecutionMode::DryRun`]): it quotes the floor price, plans the
//! eligible NACHO rebates into commit/reveal transfers, and reconciles — but
//! **never** records a txid, broadcasts, or credits a rebate. The planned
//! cycle, the planned `krc20_pending_transfer` rows, and the `krc20_cycle.plan`
//! / `krc20_cycle.reconcile` audit trail (which carries the quoted floor price)
//! are the evidence.
//!
//! This library holds the pure, deterministic part — [`RehearsalEvidence`] and
//! its [`RehearsalEvidence::to_envelope`] serializer — so the exact JSON
//! contract the operator captures is unit-tested without a database or node.

#![cfg_attr(not(test), warn(missing_docs))]
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
    )
)]

use katpool_db::repo::audit::AuditLogEntry;
use katpool_db::repo::payout::{
    Krc20EligibleWallet, Krc20PendingTransfer, Krc20TransferStatus, Payout, PayoutCycle,
    PayoutCycleStatus, PayoutKind, PayoutStatus,
};
use payout_krc20::{CreditReport, SettleReport};
use serde_json::{Value, json};

/// Crate version constant.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Stable schema identifier for the reconcile envelope.
pub const ENVELOPE_SCHEMA: &str = "katpool-krc20-rehearsal.reconcile/v1";

/// Parameters the rehearsal ran with (echoed into the envelope for provenance).
#[derive(Debug, Clone)]
pub struct RehearsalParams {
    /// Instance label used for the engine and advisory lock.
    pub instance_id: String,
    /// Schema-network identifier (`mainnet`, `testnet-10`, …).
    pub network: String,
    /// Treasury (pool) address funds would come from.
    pub treasury_address: String,
    /// Token ticker quoted and inscribed.
    pub ticker: String,
    /// Minimum pending KAS-sompi for a wallet to be selected.
    pub min_pending_sompi: i64,
    /// Minimum converted NACHO base units worth a reveal (dust gate).
    pub min_nacho_base_units: u128,
    /// KAS-sompi locked into each commit P2SH output.
    pub commit_amount_sompi: u64,
    /// Commit transaction fee (sompi).
    pub commit_fee_sompi: u64,
    /// Reveal transaction fee (sompi).
    pub reveal_fee_sompi: u64,
    /// DAA width of the payout cycle window.
    pub cycle_span_daa: u64,
    /// Virtual DAA score observed at plan time.
    pub virtual_daa: u64,
}

/// Everything gathered from one dry-run cycle, ready to serialize.
pub struct RehearsalEvidence<'a> {
    /// The parameters the rehearsal ran with.
    pub params: &'a RehearsalParams,
    /// Pre-plan snapshot of eligible wallets at the threshold.
    pub eligible: &'a [Krc20EligibleWallet],
    /// The planned cycle row.
    pub cycle: &'a PayoutCycle,
    /// The planned per-recipient `krc20_pending_transfer` rows.
    pub transfers: &'a [Krc20PendingTransfer],
    /// The parent `payout` rows (KAS-sompi being converted per recipient).
    pub payouts: &'a [Payout],
    /// Dry-run settlement report (no record, no broadcast — expected empty).
    pub settle: &'a SettleReport,
    /// Crediting report (always empty in dry-run).
    pub credit: &'a CreditReport,
    /// Cycle status after reconcile.
    pub reconciled_status: PayoutCycleStatus,
    /// Audit entries for this cycle (`krc20_cycle.plan` carries the quoted
    /// floor price; `krc20_cycle.reconcile`, …).
    pub audit: &'a [AuditLogEntry],
}

impl RehearsalEvidence<'_> {
    /// Assemble the canonical reconcile JSON envelope.
    #[must_use]
    pub fn to_envelope(&self) -> Value {
        let total_pending = self
            .eligible
            .iter()
            .fold(0_i64, |acc, w| acc.saturating_add(w.pending_sompi));

        let counts = transfer_status_histogram(self.transfers);

        json!({
            "schema": ENVELOPE_SCHEMA,
            "version": VERSION,
            "dry_run": true,
            "instance_id": self.params.instance_id,
            "network": self.params.network,
            "treasury_address": self.params.treasury_address,
            "params": {
                "ticker": self.params.ticker,
                "min_pending_sompi": self.params.min_pending_sompi,
                // u128 may exceed JSON's safe-integer range; emit as string.
                "min_nacho_base_units": self.params.min_nacho_base_units.to_string(),
                "commit_amount_sompi": self.params.commit_amount_sompi,
                "commit_fee_sompi": self.params.commit_fee_sompi,
                "reveal_fee_sompi": self.params.reveal_fee_sompi,
                "cycle_span_daa": self.params.cycle_span_daa,
                "virtual_daa": self.params.virtual_daa,
            },
            "eligible_wallets": {
                "count": self.eligible.len(),
                "total_pending_sompi": total_pending,
                "wallets": self.eligible.iter().map(eligible_to_json).collect::<Vec<_>>(),
            },
            "cycle": cycle_to_json(self.cycle),
            "transfers": self.transfers.iter().map(transfer_to_json).collect::<Vec<_>>(),
            "payouts": self.payouts.iter().map(payout_to_json).collect::<Vec<_>>(),
            "counts": {
                "total": self.transfers.len(),
                "pending": counts.pending,
                "commit_submitted": counts.commit_submitted,
                "reveal_submitted": counts.reveal_submitted,
                "completed": counts.completed,
                "failed": counts.failed,
            },
            "settle": {
                "commits_broadcast": self.settle.commits_broadcast,
                "reveals_broadcast": self.settle.reveals_broadcast,
                "rebroadcasts": self.settle.rebroadcasts,
                "completed": self.settle.completed,
                "pending": self.settle.pending,
                "errors": self.settle.errors,
            },
            "credit": {
                "credited": self.credit.credited,
                "already_credited": self.credit.already_credited,
                "paid_sompi": self.credit.paid_sompi,
            },
            "reconciled_status": cycle_status_str(self.reconciled_status),
            "audit": self.audit.iter().map(audit_to_json).collect::<Vec<_>>(),
        })
    }
}

#[derive(Default)]
struct TransferHistogram {
    pending: usize,
    commit_submitted: usize,
    reveal_submitted: usize,
    completed: usize,
    failed: usize,
}

fn transfer_status_histogram(transfers: &[Krc20PendingTransfer]) -> TransferHistogram {
    let mut h = TransferHistogram::default();
    for t in transfers {
        match t.status {
            Krc20TransferStatus::Pending => h.pending += 1,
            Krc20TransferStatus::CommitSubmitted => h.commit_submitted += 1,
            Krc20TransferStatus::RevealSubmitted => h.reveal_submitted += 1,
            Krc20TransferStatus::Completed => h.completed += 1,
            Krc20TransferStatus::Failed => h.failed += 1,
        }
    }
    h
}

fn eligible_to_json(w: &Krc20EligibleWallet) -> Value {
    json!({
        "wallet_id": w.wallet_id.0,
        "address": w.address,
        "network": w.network,
        "pending_sompi": w.pending_sompi,
    })
}

fn cycle_to_json(c: &PayoutCycle) -> Value {
    json!({
        "id": c.id,
        "kind": payout_kind_str(c.kind),
        "status": cycle_status_str(c.status),
        "daa_start": c.daa_start,
        "daa_end": c.daa_end,
        "total_sompi": c.total_sompi,
        "total_recipients": c.total_recipients,
        "idempotency_key": c.idempotency_key,
        "planned_at": c.planned_at.to_rfc3339(),
        "broadcast_at": c.broadcast_at.map(|t| t.to_rfc3339()),
        "settled_at": c.settled_at.map(|t| t.to_rfc3339()),
    })
}

fn transfer_to_json(t: &Krc20PendingTransfer) -> Value {
    json!({
        "id": t.id,
        "payout_id": t.payout_id,
        "sompi_to_miner": t.sompi_to_miner,
        "nacho_amount": t.nacho_amount,
        "p2sh_address": t.p2sh_address,
        "status": transfer_status_str(t.status),
        "created_at": t.created_at.to_rfc3339(),
        "updated_at": t.updated_at.to_rfc3339(),
    })
}

fn payout_to_json(p: &Payout) -> Value {
    json!({
        "id": p.id,
        "wallet_id": p.wallet_id.0,
        "amount_sompi": p.amount_sompi,
        "status": payout_status_str(p.status),
        "krc20_commit_hash": p.krc20_commit_hash.as_deref().map(hex_str),
        "krc20_reveal_hash": p.krc20_reveal_hash.as_deref().map(hex_str),
        "planned_at": p.planned_at.to_rfc3339(),
        "submitted_at": p.submitted_at.map(|t| t.to_rfc3339()),
        "confirmed_at": p.confirmed_at.map(|t| t.to_rfc3339()),
        "failure_reason": p.failure_reason,
    })
}

fn audit_to_json(e: &AuditLogEntry) -> Value {
    json!({
        "id": e.id.0,
        "occurred_at": e.occurred_at.to_rfc3339(),
        "actor": e.actor,
        "action": e.action,
        "subject_type": e.subject_type,
        "subject_id": e.subject_id,
        "correlation_id": e.correlation_id.map(|id| id.to_string()),
        "payload": e.payload,
    })
}

fn hex_str(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
}

const fn cycle_status_str(s: PayoutCycleStatus) -> &'static str {
    match s {
        PayoutCycleStatus::Planned => "planned",
        PayoutCycleStatus::Broadcasting => "broadcasting",
        PayoutCycleStatus::PartiallySettled => "partially_settled",
        PayoutCycleStatus::Settled => "settled",
        PayoutCycleStatus::Failed => "failed",
    }
}

const fn transfer_status_str(s: Krc20TransferStatus) -> &'static str {
    match s {
        Krc20TransferStatus::Pending => "pending",
        Krc20TransferStatus::CommitSubmitted => "commit_submitted",
        Krc20TransferStatus::RevealSubmitted => "reveal_submitted",
        Krc20TransferStatus::Completed => "completed",
        Krc20TransferStatus::Failed => "failed",
    }
}

const fn payout_status_str(s: PayoutStatus) -> &'static str {
    match s {
        PayoutStatus::Planned => "planned",
        PayoutStatus::Submitted => "submitted",
        PayoutStatus::Accepted => "accepted",
        PayoutStatus::Confirmed => "confirmed",
        PayoutStatus::Failed => "failed",
    }
}

const fn payout_kind_str(k: PayoutKind) -> &'static str {
    match k {
        PayoutKind::Kas => "kas",
        PayoutKind::Krc20Nacho => "krc20_nacho",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use katpool_db::repo::WalletId;
    use katpool_db::repo::audit::AuditLogEntry;

    fn sample_cycle() -> PayoutCycle {
        PayoutCycle {
            id: 7,
            kind: PayoutKind::Krc20Nacho,
            status: PayoutCycleStatus::Planned,
            daa_start: 0,
            daa_end: 86_400,
            planned_at: Utc.with_ymd_and_hms(2026, 5, 31, 12, 0, 0).unwrap(),
            broadcast_at: None,
            settled_at: None,
            total_sompi: 200_000_000,
            total_recipients: 1,
            idempotency_key: "krc20-0-86400".to_owned(),
        }
    }

    fn sample_transfer() -> Krc20PendingTransfer {
        Krc20PendingTransfer {
            id: 11,
            payout_id: 5,
            sompi_to_miner: 20_000_000,
            nacho_amount: 200_000_000,
            p2sh_address: "kaspatest:pqexample".to_owned(),
            status: Krc20TransferStatus::Pending,
            created_at: Utc.with_ymd_and_hms(2026, 5, 31, 12, 0, 1).unwrap(),
            updated_at: Utc.with_ymd_and_hms(2026, 5, 31, 12, 0, 1).unwrap(),
        }
    }

    fn sample_payout() -> Payout {
        Payout {
            id: 5,
            cycle_id: 7,
            wallet_id: WalletId(3),
            amount_sompi: 200_000_000,
            status: PayoutStatus::Planned,
            tx_hash: None,
            krc20_commit_hash: None,
            krc20_reveal_hash: None,
            planned_at: Utc.with_ymd_and_hms(2026, 5, 31, 12, 0, 1).unwrap(),
            submitted_at: None,
            confirmed_at: None,
            failure_reason: None,
        }
    }

    fn sample_audit() -> AuditLogEntry {
        AuditLogEntry {
            id: katpool_db::repo::AuditLogId(42),
            occurred_at: Utc.with_ymd_and_hms(2026, 5, 31, 12, 0, 2).unwrap(),
            actor: "payout-krc20".to_owned(),
            action: "krc20_cycle.plan".to_owned(),
            subject_type: Some("payout_cycle".to_owned()),
            subject_id: Some(7),
            correlation_id: None,
            payload: json!({"planned": 1, "floor_price_mantissa": "365", "floor_price_scale": 6}),
        }
    }

    #[test]
    fn envelope_has_stable_schema_and_dry_run_contract() {
        let params = RehearsalParams {
            instance_id: "katpool-krc20-rehearsal".to_owned(),
            network: "testnet-10".to_owned(),
            treasury_address: "kaspatest:qexample".to_owned(),
            ticker: "NACHO".to_owned(),
            min_pending_sompi: 100_000_000,
            min_nacho_base_units: 100_000_000,
            commit_amount_sompi: 20_000_000,
            commit_fee_sompi: 1_000_000,
            reveal_fee_sompi: 1_000_000,
            cycle_span_daa: 86_400,
            virtual_daa: 12_345,
        };
        let cycle = sample_cycle();
        let transfers = vec![sample_transfer()];
        let payouts = vec![sample_payout()];
        let settle = SettleReport::default();
        let credit = CreditReport::default();
        let audit = vec![sample_audit()];

        let evidence = RehearsalEvidence {
            params: &params,
            eligible: &[],
            cycle: &cycle,
            transfers: &transfers,
            payouts: &payouts,
            settle: &settle,
            credit: &credit,
            reconciled_status: PayoutCycleStatus::Planned,
            audit: &audit,
        };
        let env = evidence.to_envelope();

        assert_eq!(env["schema"], ENVELOPE_SCHEMA);
        assert_eq!(env["dry_run"], true);
        assert_eq!(env["network"], "testnet-10");
        assert_eq!(env["params"]["ticker"], "NACHO");
        // u128 dust gate round-trips as a string (lossless).
        assert_eq!(env["params"]["min_nacho_base_units"], "100000000");
        assert_eq!(env["cycle"]["id"], 7);
        assert_eq!(env["cycle"]["kind"], "krc20_nacho");
        assert_eq!(env["cycle"]["idempotency_key"], "krc20-0-86400");
        assert_eq!(env["counts"]["total"], 1);
        assert_eq!(env["counts"]["pending"], 1);
        assert_eq!(env["transfers"][0]["nacho_amount"], 200_000_000);
        assert_eq!(env["transfers"][0]["status"], "pending");
        assert_eq!(env["payouts"][0]["wallet_id"], 3);
        assert_eq!(env["payouts"][0]["status"], "planned");
        // Dry-run records and credits nothing.
        assert_eq!(env["settle"]["commits_broadcast"], 0);
        assert_eq!(env["credit"]["credited"], 0);
        assert_eq!(env["reconciled_status"], "planned");
        assert_eq!(env["audit"][0]["action"], "krc20_cycle.plan");
        assert_eq!(env["audit"][0]["payload"]["floor_price_mantissa"], "365");
    }
}
