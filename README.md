# POPCORN

Chain a **nodo unico** con economia nativa a **supply massima finita** — *single-operator
deterministic/verifiable execution chain*. Nessun consenso distribuito, nessun P2P: la
fiducia è sostituita dalla **verificabilità**, chiunque ri-esegue lo stato da zero e
confronta ogni `state_root`, ogni emissione e ogni burn.

> **Stato: pre-genesis.** La specifica normativa è **[`SPEC.md`](SPEC.md) — v0.9.3
> (freeze candidate)**. Finché il genesis non è prodotto, parametri e semantiche sono
> modificabili; dopo, ogni cambiamento elencato in §13 è di fatto una nuova chain.

## Le quattro garanzie (§1)

POPCORN **non** promette censorship resistance. Promette, e può dimostrare:

1. **Blindness pre-beacon** — il timelock (tlock su drand quicknet) impedisce all'operatore
   di conoscere il plaintext di un blob prima che il beacon del round esista.
2. **Ordinamento deterministico** — l'ordine di esecuzione è funzione del beacon
   (Fisher-Yates su BLAKE3-XOF), mai una scelta dell'operatore.
3. **Receipt accountability** — per ogni blob ricevutato, l'omissione dal `blob_manifest`
   è una contraddizione fra due firme dello stesso nodo.
4. **Verificabilità dello state root** — qualunque scorrettezza contabile o di esecuzione
   diverge nel replay.

Il limite è dichiarato, non nascosto: l'inclusione resta l'unico cancello, e l'accountability
copre i blob per cui il nodo ha emesso una ricevuta — non è una prova universale della
ricezione di ogni pacchetto (§1, §9.2).

## In breve

| | |
|---|---|
| Batch | 1 per round drand quicknet (3 s); `round(h) = GENESIS_DRAND_ROUND + h − 1`, mai skip |
| Identità | ed25519, `AccountId = blake3(verifying_key)` — una keypair Phantom/Solflare è valida |
| Account | impliciti alla prima ricezione di fondi; pubkey materializzata al primo spend (P2PKH) |
| Economia | fair launch (`GENESIS_SUPPLY = 0`), emissione per batch con halving, split 85/15 staker/foundation, fee **bruciate** |
| Supply | ≈ 21,02 M come limite superiore; l'effettiva la ricalcola il verificatore batch per batch |
| DEX | Uniswap V2 generalizzato ai fee tier: multi-hop, exact-in/exact-out, LP token di prima classe |
| Staking | accumulatore O(1), trascrizione letterale di Synthetix `StakingRewards` |
| HTLC | swap atomici cross-chain portati dagli utenti; hashlock SHA-256, nessun bridge di protocollo |
| Publish | bacheca dati timestampata dal beacon; nessun effetto sullo stato, nessuna VM |
| Storage | redb single-file; `blocks` append-only è la fonte di verità, `state` è cache ricostruibile |

## Il consenso è normativo (§13)

Il determinismo non è emergente ("Rust+Borsh+blake3 lo sono"): è **definito**.
`CONSENSUS_VERSION = 0x0000_0009_0002`. Ogni semantica che può influenzare lo state root —
verifica delle firme (`ed25519-dalek::verify_strict`, versione pinnata), serializzazione
Borsh, arrotondamenti AMM, calcolo delle fee, formule di emissione/reward, shuffle, mapping
round→blocco, policy del beacon — vive in §13 con la sua versione. `Cargo.lock` non è una
specifica di consenso; l'annex CONSENSUS-LOCK, impresso nel genesis, sì.

Regola ferrea che tocca ogni riga di codice consensus: **zero collezioni non ordinate nel
commitment**. Solo `BTreeMap`/`BTreeSet`/`Vec` a ordine esplicito — `HashMap`/`HashSet` sono
vietati indipendentemente dalla versione di borsh/hashbrown (rif. RUSTSEC-2024-0402).

## Gate obbligatori pre-genesis

- **Property test dello staking** (§8): milioni di sequenze emission/stake/unstake/claim,
  con verifica **dopo ogni operazione** di (a) invariante monetario a quattro bucket come
  uguaglianza esatta, (b) conservazione del totale in ogni settle, (c)
  `staking_reserved ≥ Σ pending ≥ 0`.
- **Test vector cross-language** (§2.2): encrypt/decrypt fra Rust `tlock_age`, tlock-js e
  drand/tlock Go, byte-identici — casi di **rifiuto** del profilo POPCORN-TLOCK-AGE-V1 inclusi.
- **Test vector consensus-grade** (§10): fixture end-to-end tx firmata → blob → beacon →
  batch ordinato → `state_root` → ricevuta, più un **reference executor indipendente** come
  differenziale.
- **Test di canonicità** (§2.2): stesso stato logico, ordini di inserimento diversi → byte
  identici → `state_root` identico.
- **Benchmark worst-case della raccolta cieca** (§11): 10k ciphertext validi, 10k tlock
  invalidi, 10k blob garbage.

## Obblighi operativi (fuori consenso)

L'operatore **deve** mantenere il mirror dei blob cifrati manifestati (§10, v0.9.3): senza,
l'audit della collection (`manifest → txs/rejected/unusable`) non è praticabile da terzi.
Le difese di availability della fase di raccolta — che è cieca e quindi senza fee —
sono wire-level (`MAX_TOTAL_INGRESS_PER_ROUND`, rate-limiting per IP, budget CPU di
decifratura) e non toccano il protocollo (§11).

## Naming

Il nome è un **identificativo di consenso**, non una variabile di marketing: entra nei
preimage firmati (`SIGN_DOMAIN = "popcorn-v1"`, dominio ricevuta `"popcorn-receipt-v1"`,
profili POPCORN-TLOCK-AGE-V1 / POPCORN-CONSENSUS / POPCORN-V2-MATH). Qualunque occorrenza di
ARENA / arena-chain, in qualunque forma, è stale per costruzione.
