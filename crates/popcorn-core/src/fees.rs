//! The canonical fee (SPEC.md §5.2).
//!
//! One definition, used for solvency, for the burn on `Ok` and for the burn on `Failed`.
//! Fees are collected in a single phase before execution, so no action can spend units
//! earmarked for a later transaction's fee.

use crate::constants::{FEE_TX, PUBLISH_BYTE_FEE, PUBLISH_FREE_BYTES};
use crate::types::{Action, Amount, SignedTx};

/// Fee owed by a transaction.
pub fn tx_fee(tx: &SignedTx) -> Amount {
    match &tx.payload.action {
        Action::Publish { data, .. } => {
            let billable = data.len().saturating_sub(PUBLISH_FREE_BYTES) as u128;
            FEE_TX + PUBLISH_BYTE_FEE * billable
        }
        _ => FEE_TX,
    }
}
