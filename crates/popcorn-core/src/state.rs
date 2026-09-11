//! The state, its canonical commitment (SPEC.md §5.4) and the monetary invariant (§5.5).
//!
//! Two rules shape this module and neither is negotiable:
//!
//! * only ordered collections appear anywhere near a root (§2.3) — every table is a
//!   `BTreeMap`, so traversal order is the byte order of the keys and cannot depend on
//!   insertion history;
//! * a balance reaching zero is removed rather than stored (§14.1), otherwise two logically
//!   identical states would hash differently.

use std::collections::BTreeMap;

use borsh::BorshSerialize;

use crate::constants::NATIVE_TOKEN;
use crate::types::{Account, AccountId, Amount, Global, Htlc, Pair, Token, TokenId};

/// Table tags, in the frozen traversal order of §5.4.
const TAG_ACCOUNTS: u8 = 0x01;
const TAG_TOKENS: u8 = 0x02;
const TAG_PAIRS: u8 = 0x03;
const TAG_HTLCS: u8 = 0x04;
const TAG_GLOBAL: u8 = 0x05;

/// The full consensus state.
///
/// `hashlock_index` is deliberately not part of it: see [`State::hashlock_index`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct State {
    pub accounts: BTreeMap<AccountId, Account>,
    pub tokens: BTreeMap<TokenId, Token>,
    pub pairs: BTreeMap<[u8; 32], Pair>,
    pub htlcs: BTreeMap<[u8; 32], Htlc>,
    pub global: Global,
}

/// Undo record for one mutation, used to roll a failed transaction back (§5.2).
#[derive(Clone, Debug)]
enum Undo {
    Account(AccountId, Option<Account>),
    Token(TokenId, Option<Token>),
    Pair([u8; 32], Option<Pair>),
    Htlc([u8; 32], Option<Htlc>),
    Global(Global),
}

/// A rollback journal: previous values of everything a transaction touched.
///
/// Cloning the whole state per transaction would be correct but quadratic; recording the
/// previous value of each touched key is proportional to what the action actually wrote.
#[derive(Debug, Default)]
pub struct Journal {
    entries: Vec<Undo>,
}

impl Journal {
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of recorded mutations. Used by tests to assert that reads do not journal.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn record(&mut self, undo: Undo) {
        self.entries.push(undo);
    }
}

impl State {
    pub fn new() -> Self {
        Self::default()
    }

    // -----------------------------------------------------------------------------------
    // Journalled mutation
    // -----------------------------------------------------------------------------------

    /// Roll back every mutation recorded in `journal`, most recent first.
    ///
    /// Applying in reverse makes repeated writes to the same key collapse correctly: the
    /// last restore wins, and it is the oldest recorded value.
    pub fn rollback(&mut self, journal: Journal) {
        for undo in journal.entries.into_iter().rev() {
            match undo {
                Undo::Account(id, prev) => match prev {
                    Some(a) => {
                        self.accounts.insert(id, a);
                    }
                    None => {
                        self.accounts.remove(&id);
                    }
                },
                Undo::Token(id, prev) => match prev {
                    Some(t) => {
                        self.tokens.insert(id, t);
                    }
                    None => {
                        self.tokens.remove(&id);
                    }
                },
                Undo::Pair(id, prev) => match prev {
                    Some(p) => {
                        self.pairs.insert(id, p);
                    }
                    None => {
                        self.pairs.remove(&id);
                    }
                },
                Undo::Htlc(id, prev) => match prev {
                    Some(h) => {
                        self.htlcs.insert(id, h);
                    }
                    None => {
                        self.htlcs.remove(&id);
                    }
                },
                Undo::Global(g) => self.global = g,
            }
        }
    }

    fn touch_account(&mut self, id: &AccountId, journal: &mut Journal) {
        journal.record(Undo::Account(*id, self.accounts.get(id).cloned()));
    }

    fn touch_global(&mut self, journal: &mut Journal) {
        journal.record(Undo::Global(self.global.clone()));
    }

    /// Create the account if it does not exist yet (§4.2) and return a mutable handle.
    ///
    /// This is the only place an account is born, and the only place `account_count` grows.
    pub fn account_entry(&mut self, id: &AccountId, journal: &mut Journal) -> &mut Account {
        self.touch_account(id, journal);
        if !self.accounts.contains_key(id) {
            self.touch_global(journal);
            self.global.account_count += 1;
            self.accounts.insert(*id, Account::default());
        }
        self.accounts.get_mut(id).expect("account just ensured")
    }

    /// Mutable handle to an existing account, without creating one.
    pub fn account_mut(&mut self, id: &AccountId, journal: &mut Journal) -> Option<&mut Account> {
        if !self.accounts.contains_key(id) {
            return None;
        }
        self.touch_account(id, journal);
        self.accounts.get_mut(id)
    }

    pub fn account(&self, id: &AccountId) -> Option<&Account> {
        self.accounts.get(id)
    }

    /// Mutable handle to the global singleton.
    pub fn global_mut(&mut self, journal: &mut Journal) -> &mut Global {
        self.touch_global(journal);
        &mut self.global
    }

    pub fn insert_token(&mut self, token: Token, journal: &mut Journal) {
        journal.record(Undo::Token(token.id, self.tokens.get(&token.id).cloned()));
        self.tokens.insert(token.id, token);
    }

    pub fn insert_pair(&mut self, pair: Pair, journal: &mut Journal) {
        journal.record(Undo::Pair(pair.id, self.pairs.get(&pair.id).cloned()));
        self.pairs.insert(pair.id, pair);
    }

    pub fn pair_mut(&mut self, id: &[u8; 32], journal: &mut Journal) -> Option<&mut Pair> {
        if !self.pairs.contains_key(id) {
            return None;
        }
        journal.record(Undo::Pair(*id, self.pairs.get(id).cloned()));
        self.pairs.get_mut(id)
    }

    pub fn insert_htlc(&mut self, htlc: Htlc, journal: &mut Journal) {
        journal.record(Undo::Htlc(htlc.id, self.htlcs.get(&htlc.id).cloned()));
        self.htlcs.insert(htlc.id, htlc);
    }

    pub fn remove_htlc(&mut self, id: &[u8; 32], journal: &mut Journal) -> Option<Htlc> {
        journal.record(Undo::Htlc(*id, self.htlcs.get(id).cloned()));
        self.htlcs.remove(id)
    }

    // -----------------------------------------------------------------------------------
    // Balances
    // -----------------------------------------------------------------------------------

    /// Balance of `token` for `id`; zero for accounts and entries that do not exist.
    pub fn balance_of(&self, id: &AccountId, token: &TokenId) -> Amount {
        self.accounts
            .get(id)
            .and_then(|a| a.balances.get(token))
            .copied()
            .unwrap_or(0)
    }

    /// Credit `amount` to `id`, creating the account if needed (§4.2).
    ///
    /// A zero credit is a no-op and does not bring an account into existence.
    pub fn credit(
        &mut self,
        id: &AccountId,
        token: &TokenId,
        amount: Amount,
        journal: &mut Journal,
    ) -> Result<(), ()> {
        if amount == 0 {
            return Ok(());
        }
        let account = self.account_entry(id, journal);
        let entry = account.balances.entry(*token).or_insert(0);
        *entry = entry.checked_add(amount).ok_or(())?;
        Ok(())
    }

    /// Debit `amount` from `id`, failing if the balance is insufficient.
    ///
    /// An entry hitting zero is removed, keeping the encoding canonical (§14.1).
    pub fn debit(
        &mut self,
        id: &AccountId,
        token: &TokenId,
        amount: Amount,
        journal: &mut Journal,
    ) -> Result<(), ()> {
        if amount == 0 {
            return Ok(());
        }
        let Some(account) = self.account_mut(id, journal) else {
            return Err(());
        };
        let balance = account.balances.get(token).copied().unwrap_or(0);
        if balance < amount {
            return Err(());
        }
        let remaining = balance - amount;
        if remaining == 0 {
            account.balances.remove(token);
        } else {
            account.balances.insert(*token, remaining);
        }
        Ok(())
    }

    /// Burn native units: debit and account for them in `native_burned` (§7.4).
    pub fn burn_native(
        &mut self,
        id: &AccountId,
        amount: Amount,
        journal: &mut Journal,
    ) -> Result<(), ()> {
        self.debit(id, &NATIVE_TOKEN, amount, journal)?;
        let global = self.global_mut(journal);
        global.native_burned = global.native_burned.checked_add(amount).ok_or(())?;
        Ok(())
    }

    // -----------------------------------------------------------------------------------
    // Commitment
    // -----------------------------------------------------------------------------------

    /// Canonical state root (§5.4).
    ///
    /// Tables are visited in tag order; inside each, keys in lexicographic byte order of
    /// their Borsh encoding; each key and value is length-prefixed with LE32 so no
    /// concatenation is ambiguous.
    pub fn state_root(&self) -> [u8; 32] {
        let mut h = blake3::Hasher::new();

        h.update(&[TAG_ACCOUNTS]);
        for (k, v) in &self.accounts {
            hash_entry(&mut h, k, v);
        }

        h.update(&[TAG_TOKENS]);
        for (k, v) in &self.tokens {
            hash_entry(&mut h, k, v);
        }

        h.update(&[TAG_PAIRS]);
        for (k, v) in &self.pairs {
            hash_entry(&mut h, k, v);
        }

        h.update(&[TAG_HTLCS]);
        for (k, v) in &self.htlcs {
            hash_entry(&mut h, k, v);
        }

        // The singleton is encoded as a single pair with an empty key, so a non-Rust
        // implementer has nothing to infer (§5.4).
        h.update(&[TAG_GLOBAL]);
        let bv = borsh::to_vec(&self.global).expect("borsh serialization of Global");
        h.update(&0u32.to_le_bytes());
        h.update(&(bv.len() as u32).to_le_bytes());
        h.update(&bv);

        *h.finalize().as_bytes()
    }

    // -----------------------------------------------------------------------------------
    // Derived views
    // -----------------------------------------------------------------------------------

    /// The `hashlock → htlc_id` index used by auto-settlement (§7.6).
    ///
    /// Derived, never serialized and never part of the state root: it cannot diverge
    /// without the committed `htlcs` table diverging first.
    pub fn hashlock_index(&self) -> BTreeMap<[u8; 32], [u8; 32]> {
        let mut index = BTreeMap::new();
        for (id, htlc) in &self.htlcs {
            index.insert(htlc.hashlock, *id);
        }
        index
    }

    /// Total native units sitting in liquid balances.
    pub fn total_native_balances(&self) -> Amount {
        self.accounts
            .values()
            .map(|a| a.balances.get(&NATIVE_TOKEN).copied().unwrap_or(0))
            .sum()
    }

    /// Total native units under stake.
    pub fn total_staked_sum(&self) -> Amount {
        self.accounts.values().map(|a| a.staked).sum()
    }

    /// Total native units escrowed in open HTLCs.
    pub fn total_native_in_htlcs(&self) -> Amount {
        self.htlcs
            .values()
            .filter(|h| h.token == NATIVE_TOKEN)
            .map(|h| h.amount)
            .sum()
    }

    /// The four-bucket monetary invariant of §5.5, as an exact equality.
    ///
    /// Every native unit is in exactly one of: a liquid balance, stake, HTLC escrow, or the
    /// staking liability. Replay checks this at every block.
    pub fn monetary_invariant_holds(&self, genesis_supply: Amount) -> bool {
        let left = self.total_native_balances()
            + self.total_staked_sum()
            + self.total_native_in_htlcs()
            + self.global.staking_reserved;
        let right = genesis_supply + self.global.native_emitted - self.global.native_burned;
        left == right
    }

    /// Sum of every account's claimable reward (§8). O(N): for tests and audits, never on
    /// the execution path.
    pub fn total_pending(&self) -> Amount {
        self.accounts
            .values()
            .map(|a| crate::staking::pending(a, self.global.acc_per_stake))
            .sum()
    }
}

fn hash_entry<K: BorshSerialize, V: BorshSerialize>(h: &mut blake3::Hasher, key: &K, value: &V) {
    let bk = borsh::to_vec(key).expect("borsh serialization of a state key");
    let bv = borsh::to_vec(value).expect("borsh serialization of a state value");
    h.update(&(bk.len() as u32).to_le_bytes());
    h.update(&bk);
    h.update(&(bv.len() as u32).to_le_bytes());
    h.update(&bv);
}
