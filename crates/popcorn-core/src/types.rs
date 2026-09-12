//! Consensus data structures (SPEC.md §4.3) and the normative enum discriminants (§13.1).
//!
//! Borsh assigns enum discriminants by declaration order, and `results_root` and
//! `rejected_root` commit to those values: adding, removing or reordering a variant is
//! consensus-breaking. The order below is the tabulated one and must not change.

use std::collections::BTreeMap;

use borsh::{BorshDeserialize, BorshSerialize};

/// Account identifier: `blake3(verifying_key)`.
pub type AccountId = [u8; 32];
/// Token, LP token or native token identifier.
pub type TokenId = [u8; 32];
/// Every amount in the protocol, at 9 decimals.
pub type Amount = u128;

/// An account record. Born implicitly on first receipt of funds (§4.2).
#[derive(Clone, Debug, Default, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Account {
    /// `None` until the account's first executed signed transaction materializes it (§4.2).
    pub pubkey: Option<[u8; 32]>,
    /// Last executed nonce; starts at 0, so the first usable nonce is 1.
    pub nonce: u64,
    /// Token balances. A balance reaching zero is removed, never stored (§14.1).
    pub balances: BTreeMap<TokenId, Amount>,
    /// Native units currently staked. Not spendable, not transferable (§8).
    pub staked: Amount,
    /// Snapshot of `acc_per_stake` at the last settle — Synthetix `userRewardPerTokenPaid`.
    pub paid_acc: u128,
}

/// A user-created token (§7.3).
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Token {
    pub id: TokenId,
    pub creator: AccountId,
    /// 16 bytes of printable ASCII, right zero-padded. Names are not unique.
    pub name: [u8; 16],
    pub total_supply: Amount,
}

/// An AMM pair (§6).
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Pair {
    pub id: [u8; 32],
    /// Lexicographically smaller side.
    pub token0: TokenId,
    pub token1: TokenId,
    pub fee_bps: u16,
    pub reserve0: Amount,
    pub reserve1: Amount,
    pub lp_supply: Amount,
}

/// A hashed timelock contract (§7.6).
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Htlc {
    pub id: [u8; 32],
    pub sender: AccountId,
    /// Fixed at lock time and immutable.
    pub recipient: AccountId,
    pub token: TokenId,
    pub amount: Amount,
    /// `sha256(preimage)` — SHA-256, not blake3, for cross-chain interoperability.
    pub hashlock: [u8; 32],
    pub expiry_round: u64,
}

/// Global singleton state. Field order is frozen: it is hashed as-is (§5.4).
///
/// The first two fields are the chain's identity, written at genesis and never touched
/// again. They are here rather than in the header or in a config file for one reason: this
/// struct is inside `state_root`, so the rules a chain was created under are committed to by
/// every block, and a replay under different rules diverges at block 0 instead of quietly
/// producing a different history. A verifier holding only the exported blocks can check it.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Global {
    /// `CONSENSUS_VERSION` as of genesis (§13). Immutable for the life of the chain.
    pub consensus_version: u64,
    /// Digest of the pinned dependency list of §13. Immutable for the life of the chain.
    pub lock_digest: [u8; 32],
    pub height: u64,
    pub total_staked: Amount,
    pub acc_per_stake: u128,
    /// Staking liability: emitted units not yet moved into a balance (§5.5).
    pub staking_reserved: Amount,
    pub native_emitted: Amount,
    pub native_burned: Amount,
    pub account_count: u64,
}

/// A fresh `Global` carries this binary's identity, not zeros.
///
/// `Default` is what builds the genesis state, so zeroing these two would stamp an empty
/// identity into the chain and make the check meaningless. Deserializing a stored state
/// never goes through here, so an existing chain keeps whatever it was stamped with — which
/// is exactly how a mismatched binary gets caught.
impl Default for Global {
    fn default() -> Self {
        Self {
            consensus_version: crate::constants::CONSENSUS_VERSION,
            lock_digest: crate::crypto::consensus_lock_digest(),
            height: 0,
            total_staked: 0,
            acc_per_stake: 0,
            staking_reserved: 0,
            native_emitted: 0,
            native_burned: 0,
            account_count: 0,
        }
    }
}

/// The action a transaction performs. Discriminant order is normative (§13.1).
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Action {
    Transfer {
        token: TokenId,
        to: AccountId,
        amount: Amount,
    },
    CreateToken {
        name: [u8; 16],
        supply: Amount,
    },
    CreatePair {
        token_a: TokenId,
        token_b: TokenId,
        fee_bps: u16,
    },
    AddLiquidity {
        pair: [u8; 32],
        amount0_desired: Amount,
        amount1_desired: Amount,
        amount0_min: Amount,
        amount1_min: Amount,
    },
    RemoveLiquidity {
        pair: [u8; 32],
        lp_amount: Amount,
        amount0_min: Amount,
        amount1_min: Amount,
    },
    SwapExactIn {
        path: Vec<[u8; 32]>,
        token_in: TokenId,
        amount_in: Amount,
        min_amount_out: Amount,
    },
    SwapExactOut {
        path: Vec<[u8; 32]>,
        token_in: TokenId,
        amount_out: Amount,
        max_amount_in: Amount,
    },
    /// A data board entry: no state effect, the data lives only in the block (§7.5).
    Publish {
        topic: [u8; 32],
        data: Vec<u8>,
    },
    HtlcLock {
        to: AccountId,
        token: TokenId,
        amount: Amount,
        hashlock: [u8; 32],
        expiry_round: u64,
    },
    HtlcClaim {
        htlc_id: [u8; 32],
        preimage: [u8; 32],
    },
    /// Invocable by anyone once expired: state garbage collection (§7.6).
    HtlcRefund {
        htlc_id: [u8; 32],
    },
    Stake {
        amount: Amount,
    },
    Unstake {
        amount: Amount,
    },
    ClaimRewards {},
}

/// The signed part of a transaction.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct TxPayload {
    pub nonce: u64,
    pub target_round: u64,
    pub action: Action,
}

/// A transaction as transmitted and executed.
///
/// `tx_id` covers this whole structure, signature included: the same intent signed twice
/// yields two ids, and nonce deduplication (§5.2) decides which one executes.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct SignedTx {
    pub payload: TxPayload,
    /// ed25519 verifying key; `signer = blake3(signer_pubkey)`, never a reverse lookup.
    pub signer_pubkey: [u8; 32],
    pub signature: [u8; 64],
}

impl SignedTx {
    /// `tx_id = blake3(borsh(SignedTx))` (§4.1).
    pub fn tx_id(&self) -> [u8; 32] {
        let encoded = borsh::to_vec(self).expect("borsh serialization of a SignedTx");
        crate::crypto::blake3_hash(&encoded)
    }

    /// The account this transaction is signed by, derived from the carried key.
    pub fn signer(&self) -> AccountId {
        crate::crypto::account_id_from_pubkey(&self.signer_pubkey)
    }
}

/// Why a transaction never executed. Zero fee, nonce untouched (§5.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
#[borsh(use_discriminant = true)]
pub enum RejectReason {
    /// Reserved, and unreachable by construction: a blob whose decrypted cleartext does not
    /// Borsh-decode is `unusable` (§5.1), never `rejected` — no `tx_id` exists to reject. The
    /// discriminant is kept at 0 rather than removed, because renumbering the others would
    /// change `rejected_root` (§13.1).
    Malformed = 0,
    BadSignature = 1,
    WrongRound = 2,
    UnknownAccount = 3,
    PubkeyMismatch = 4,
    FieldOutOfRange = 5,
    DuplicateNonce = 6,
    NonceGap = 7,
    OverBudget = 8,
    FeeInsolvent = 9,
    NonceExhausted = 10,
}

/// Why an executed transaction rolled back. Fee burned, nonce consumed (§5.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
#[borsh(use_discriminant = true)]
pub enum FailReason {
    InsufficientBalance = 0,
    SlippageExceeded = 1,
    UnknownToken = 2,
    UnknownPair = 3,
    PairAlreadyExists = 4,
    LpTokenAsPairSide = 5,
    ZeroOutput = 6,
    LiquidityTooSmall = 7,
    ReGenesisGuard = 8,
    BadPath = 9,
    StakeLiquidityGuard = 10,
    Overflow = 11,
    /// Reserved and unreachable: static validation catches it first with `FieldOutOfRange`.
    SupplyOutOfRange = 12,
    SelfTransferNoop = 13,
    HtlcNotFound = 14,
    HtlcBadPreimage = 15,
    HtlcExpired = 16,
    HtlcNotExpired = 17,
    HtlcDuplicateHashlock = 18,
}

/// Execution outcome, index-aligned with `Block.txs`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum ExecStatus {
    Ok,
    Failed(FailReason),
}

/// The committed, signed object. `Block` is a container bound by these roots.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Header {
    pub height: u64,
    /// `block_hash` of the previous block; `[0; 32]` at genesis.
    pub prev_hash: [u8; 32],
    pub drand_round: u64,
    pub drand_sig_hash: [u8; 32],
    pub collection_root: [u8; 32],
    pub txs_root: [u8; 32],
    pub rejected_root: [u8; 32],
    pub results_root: [u8; 32],
    pub state_root: [u8; 32],
}

impl Header {
    /// `block_hash = blake3(borsh(Header))` (§4.3).
    pub fn block_hash(&self) -> [u8; 32] {
        let encoded = borsh::to_vec(self).expect("borsh serialization of a Header");
        crate::crypto::blake3_hash(&encoded)
    }
}

/// A full block as served and replayed.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Block {
    pub header: Header,
    /// Full BLS signature of the round's beacon, kept for verification and tlock replay.
    pub drand_signature: Vec<u8>,
    /// blake3 of every blob received for this round, lexicographic. A set, not a multiset.
    pub blob_manifest: Vec<[u8; 32]>,
    /// Derived evidence (§5.1): manifest entries that yielded no `SignedTx`.
    pub unusable: Vec<[u8; 32]>,
    /// Execution order.
    pub txs: Vec<SignedTx>,
    /// Index-aligned with `txs`.
    pub results: Vec<ExecStatus>,
    /// Lexicographically sorted by `tx_id`.
    pub rejected: Vec<([u8; 32], RejectReason)>,
    pub node_signature: [u8; 64],
}

/// What a submission receipt signs (§9.2). The only normative preimage.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct ReceiptPayload {
    /// Always `RECEIPT_DOMAIN`; carried explicitly so the encoding is self-describing.
    pub domain: String,
    pub blob_hash: [u8; 32],
    pub target_round: u64,
    /// Node-declared and non-consensus: never used for ordering, validity or proof of time.
    pub timestamp_ms: u64,
}

/// A signed receipt as returned by `POST /tx`.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Receipt {
    pub payload: ReceiptPayload,
    pub node_pubkey: [u8; 32],
    pub signature: [u8; 64],
}

impl ReceiptPayload {
    /// `receipt_hash = blake3(borsh(ReceiptPayload))`.
    pub fn receipt_hash(&self) -> [u8; 32] {
        let encoded = borsh::to_vec(self).expect("borsh serialization of a ReceiptPayload");
        crate::crypto::blake3_hash(&encoded)
    }
}
