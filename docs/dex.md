[← Docs](README.md) · [Overview](overview.md) · [Architecture](architecture.md) · **DEX** · [HTLC](htlc.md) · [Publish](publish.md) · [Staking](staking.md) · [Security](security.md) · [API](api.md) · [Web](web.md) · [Node](running-a-node.md) · [Verification](verification.md)

# DEX

> Uniswap V2, generalized to fee tiers and multi-hop. Every intermediate in `U256`, every
> division a floor, every rounding toward the pool.
> *Specification: [§6](../SPEC.md#6-amm-math--popcorn-v2-math)*

## What exists

| | |
|---|---|
| Pairs | any two distinct tokens, `NATIVE` included — never two LP tokens |
| Fee tiers | 5, 30 or 100 bps; the tier is part of the pair's identity, so the same couple can have three pools |
| Routing | multi-hop up to 4 pairs, atomic end to end |
| Directions | exact-in (`SwapExactIn`) and exact-out (`SwapExactOut`) |
| LP tokens | first-class balances — `Transfer` works on them with no dedicated code |
| Swap fees | stay with the LPs of the hop that earned them. Only the flat transaction fee is burned |

## The math

**Exact-in**, per hop:

```
amount_in_with_fee = amount_in × (10_000 − fee_bps)
amount_out = (amount_in_with_fee × reserve_out)
           / (reserve_in × 10_000 + amount_in_with_fee)
```

**Exact-out**, per hop:

```
require(amount_out < reserve_out)        ← asking for the whole reserve has no finite price
amount_in = (reserve_in × amount_out × 10_000)
          / ((reserve_out − amount_out) × (10_000 − fee_bps)) + 1
```

That `+ 1` is the pool-favouring correction. It is why a round trip is never profitable, and
the test suite checks exactly that: exact-out never asks for less than exact-in would have
returned.

**Multi-hop exact-out** runs backward first — the last hop's requirement becomes the previous
hop's output, all the way to the front — and then executes **forward with exactly those
amounts**, never recomputed. Recomputing mid-execution would price the same swap twice.

## Liquidity lifecycle

```
genesis      reserves 0, no LP:      liquidity = √(amount0 × amount1) − MINIMUM_LIQUIDITY
                                     MINIMUM_LIQUIDITY is credited to supply but to no account

re-genesis   reserves 0, LP == MINIMUM_LIQUIDITY:
                                     the pair restarts on the genesis formula

guard        reserves 0, LP > MINIMUM_LIQUIDITY:  Failed(ReGenesisGuard)

subsequent   liquidity = min(amount0 × lp_supply / reserve0,
                             amount1 × lp_supply / reserve1)

removal      amount_i = lp_amount × reserve_i / lp_supply
```

The re-genesis guard is not bookkeeping pedantry. Residual LP tokens against zero reserves
would hold a claim on nothing, and diluting new depositors in favour of that claim is exactly
the trap the guard refuses. Burn the residue first with a zero-yield `RemoveLiquidity`, and the
pair restarts clean.

## Deposits take only what they need

`AddLiquidity` follows Router02: it computes the **actual** amounts at the pool's current ratio
and debits only those. The excess you offered is never touched.

```
a1_opt = ⌊amount0_desired × reserve1 / reserve0⌋
if a1_opt ≤ amount1_desired:  (actual0, actual1) = (amount0_desired, a1_opt)
else:                          (actual0, actual1) = (⌊amount1_desired × reserve0 / reserve1⌋,
                                                     amount1_desired)
require(actual0 ≥ amount0_min && actual1 ≥ amount1_min)
```

Slippage bounds apply to the actuals, not to what you asked for.

## Rules that refuse rather than shrug

| Situation | Outcome |
|---|---|
| A hop would yield zero | `Failed(ZeroOutput)` — no paying a fee to move dust |
| Both sides of a pair are the same token | `Failed(BadPath)` |
| An LP token as a pair side | `Failed(LpTokenAsPairSide)` |
| The pair already exists | `Failed(PairAlreadyExists)` |
| A path hop does not hold the current token | `Failed(BadPath)` |
| A half-empty pool (one reserve zero) | `Failed(ReGenesisGuard)` — no defined price |

The constant-product invariant `k_after ≥ k_before` is claimed **only across successful swap
hops**. Adding and removing liquidity change `k` by definition, and pretending otherwise would
make the claim meaningless.

## Native pools and the monetary invariant

A pair can hold the native token, and those units left somebody's balance to get there. They
are a **bucket of their own** in the monetary invariant of
[§5.5](../SPEC.md#55-five-bucket-monetary-invariant-exact-equality-by-construction) — see
[Staking & economy](staking.md#where-every-native-unit-lives).

This is worth a sentence of history: the invariant originally listed four buckets and forgot
pool reserves, which made it false for any chain with native liquidity. Since the verifier
checks it at every block, an honest chain would have failed its own verification. The
independent reference executor found it; the details are in [Verification](verification.md).

## Using it

```bash
popcorn submit --key alice.key --node http://127.0.0.1:8080 \
    create-token --name POPTEST --supply 1000000000000     # prints the token id

popcorn submit --key alice.key --node http://127.0.0.1:8080 \
    create-pair --token-a native --token-b <TOKEN> --fee-bps 30   # prints the pair id

popcorn submit --key alice.key --node http://127.0.0.1:8080 \
    add-liquidity --pair <PAIR> --amount0 1000000000 --amount1 50000000000

popcorn submit --key alice.key --node http://127.0.0.1:8080 \
    swap-in --path <PAIR> --token-in native --amount-in 100000000 --min-out 1

popcorn submit --key alice.key --node http://127.0.0.1:8080 \
    swap-out --path <PAIR1>,<PAIR2> --token-in native --amount-out 5000 --max-in 100000
```

Token and pair ids derive from the signer and the nonce, so the client prints the id a
transaction *will* produce — you cannot look up a token that does not exist yet.

---

[← Architecture](architecture.md) · Next: [HTLC →](htlc.md)
