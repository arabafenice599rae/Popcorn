[← Docs](README.md) · [Overview](overview.md) · [Architecture](architecture.md) · [DEX](dex.md) · [HTLC](htlc.md) · [Publish](publish.md) · **Staking** · [Security](security.md) · [API](api.md) · [Web](web.md) · [Node](running-a-node.md) · [Verification](verification.md)

# Staking & economy

> Nothing is allocated at genesis. Every native unit that will ever exist is minted by the
> emission formula, and the whole supply is reconstructible by replaying the chain.
> *Specification: [§7](../SPEC.md#7-economics), [§8](../SPEC.md#8-staking--o1-distribution)*

## Fair launch

`GENESIS_SUPPLY = 0`. No premine, no primary sale, no faucet, no invites. Block 0 mints
nothing; the first native units appear when block 1 closes.

The `FOUNDATION` account is an ordinary account holding the operator's key, and its movements
are as public as anyone's. It receives 15% of each emission, which is the bootstrap: the first
tokens in circulation are its share, distributed as grants, payments or pool liquidity so that
other people can transact at all.

The foundation may also stake and earn like anyone else. That is a deliberate choice rather
than an oversight — there is no special case in the code, and the resulting concentration is
readable on-chain by anybody who cares to look.

## Emission

```
emission_index   = height − 1                                   ← 0-based, so epochs are exact
EMISSION(height) = EMISSION_0 >> (emission_index / HALVING_INTERVAL)

staker_share     = EMISSION × 8_500 / 10_000                    ← 85%
foundation_share = EMISSION − staker_share                      ← 15%, exactly
```

| Parameter | Value | |
|---|---|---|
| `EMISSION_0` | 1 × 10⁹ | one native per batch, ≈ 28,800/day |
| `HALVING_INTERVAL` | 10,512,000 batches | ≈ one year at 3 s |
| Upper bound | ≈ 21.02 M | the effective figure is lower — see below |

**When nothing is staked, the staker share is not born.** It is not redirected to the
foundation and it is not burned; it simply never exists. So the foundation is capped at its
nominal 15% from the very first batch, even while it is briefly receiving 100% of what is
actually minted. Both statements are true at once, and the spec says so rather than picking the
flattering one.

## Fees are burned

A flat `FEE_TX` of 5,000 units per executed transaction — `Ok` and `Failed` alike — burned
outright. Rejected transactions pay nothing. A multi-hop swap pays one flat fee however many
pools it crosses.

The operator earns nothing from fees. Against a decreasing emission, a chain that gets used
can become net deflationary.

## The staking accumulator

O(1) distribution, transcribed literally from the audited Synthetix `StakingRewards`:

```
per batch:    acc_per_stake    += staker_share × PRECISION / total_staked
              staking_reserved += staker_share                   ← WHOLE, no dust

per account:  pending = ⌊staked × (acc_per_stake − paid_acc) / PRECISION⌋
                        └─────── one floor, over the difference ───────┘

settle:       1. pay pending as a TRANSFER: reserved −= p; balance += p
              2. then update staked
              3. then snapshot paid_acc = acc_per_stake
```

The shape matters more than it looks. The reserve takes the share **whole** and the only
flooring is user-side, over the accumulator *difference*. That is what makes

```
staking_reserved ≥ Σ pending ≥ 0
```

true by construction rather than by hope. An earlier design floored twice over different bases
and routed the remainder to the foundation; the first claim after such a payout could underflow
the reserve and halt the chain. The repository keeps a test that models that superseded rule
and shows the claims outrunning the reserve — a gate nobody has seen fail is a gate nobody
trusts.

What remains is a **rounding residue**: `staking_reserved − Σ pending`, the fractions user-side
flooring leaves behind. It is a protocol liability, not attributable without O(N) work, so it
is neither burned nor gifted. It stays, declared.

## The liquidity guard

`Stake` requires that at least one `FEE_TX` stays liquid afterwards. Without it, an account
staking its entire balance could never pay the fee for its own `Unstake` — value locked in
forever by an action that looked reasonable at the time.

There is no unbonding period. Stake is neither spendable nor transferable while staked, and
only the native token is stakeable. Staking is the only way to take part in emission: it is
this chain's mining.

## Where every native unit lives

```
  Σ balances[NATIVE]                  liquid, spendable
+ Σ staked                            locked in stake
+ Σ htlcs[NATIVE].amount              escrowed in open locks
+ Σ pairs[NATIVE side].reserve        held as AMM reserves
+ staking_reserved                    minted as rewards, not yet claimed
  ────────────────────────────────
= GENESIS_SUPPLY + native_emitted − native_burned
```

Five places, exactly one each, no unit living inside a formula. The verifier checks this
equality at **every block**, and `GET /supply` reports all five buckets with their total so you
can check it without joining two endpoints by hand.

> The fifth bucket is a correction, not original text. The invariant listed four and omitted
> pool reserves, which made it false for any chain with native liquidity — and since the
> verifier checks it every block, an honest chain would have been told it was incorrect. See
> [Verification](verification.md#what-the-gates-found).

## Using it

```bash
popcorn submit --key alice.key --node http://127.0.0.1:8080 stake   --amount 500000000
popcorn submit --key alice.key --node http://127.0.0.1:8080 claim
popcorn submit --key alice.key --node http://127.0.0.1:8080 unstake --amount 500000000
```

`GET /account/{id}` reports `staked` and `pending_rewards`; `GET /supply` reports the buckets.

---

[← Publish](publish.md) · Next: [Security →](security.md)
