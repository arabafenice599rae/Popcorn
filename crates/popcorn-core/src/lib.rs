//! POPCORN consensus core.
//!
//! Everything that can influence the state root lives in this crate, and nothing else does:
//! no I/O, no clock, no network. That separation is what makes the two things §10 asks for
//! possible — a replay verifier and an independent reference executor — and it is why the
//! node crate can be rewritten without touching consensus.
//!
//! The normative document is `SPEC.md` at the repository root; section references in these
//! modules point at it.

pub mod amm;
pub mod constants;
pub mod crypto;
pub mod emission;
pub mod execute;
pub mod fees;
pub mod genesis;
pub mod ids;
pub mod shuffle;
pub mod staking;
pub mod state;
pub mod types;
pub mod validate;

pub use constants::{CONSENSUS_VERSION, NATIVE_TOKEN, SIGN_DOMAIN};
pub use state::{Journal, State};
pub use types::{
    Account, AccountId, Action, Amount, Block, ExecStatus, FailReason, Global, Header, Htlc, Pair,
    Receipt, ReceiptPayload, RejectReason, SignedTx, Token, TokenId, TxPayload,
};
