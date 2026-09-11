# POPCORN — Specifica Tecnica v0.9.3 (freeze candidate)

**Rename (pre-genesis)**: la chain si chiama **POPCORN** (già ARENA-CHAIN). Rinominate coerentemente anche le costanti di consenso che contengono il nome: `SIGN_DOMAIN = "popcorn-v1"` (10 byte), dominio ricevuta `"popcorn-receipt-v1"`, profili POPCORN-TLOCK-AGE-V1 / POPCORN-CONSENSUS / POPCORN-V2-MATH. Pre-genesis il rename è libero; post-genesis sarebbe stato consensus-breaking (i domini entrano nei preimage firmati). **Il nome è un identificativo di consenso, non una variabile di marketing: qualunque occorrenza futura di ARENA/arena-chain, in qualunque forma, è stale per costruzione.**

Chain a nodo unico con economia nativa a **supply massima finita**.

**Changelog v0.9.3 — coerenza finale**: eliminate le 3 menzioni stale della dust che contraddicevano la v0.9.2 (§7.1, §7.2 `native_emitted`, §13); pinnati i 3 casi residui — `amount > 0` statico per `Transfer`/`Stake`/`Unstake`, sotto-ordine del passo 4 (`PubkeyMismatch` prima di `NonceExhausted`), `amount_out ≥ reserve_out` in exact-out ⇒ `Failed(SlippageExceeded)`; `CONSENSUS_VERSION` in forma a tre campi 16-bit; mirror dei blob promosso da opzionale a **obbligo operativo** dell'operatore (fuori consenso), necessario perché l'audit della collection sia praticabile.

**Changelog v0.9.2 — staking = Synthetix letterale (fix del P0 su `reserved ≥ Σ pending`)**: la disequazione dichiarata in v0.9.1 era falsa (controesempio: la dust girata a foundation era esattamente ciò che i pending avrebbero reclamato → primo claim in underflow → halt). Fix per trascrizione **letterale della matematica auditata di Synthetix StakingRewards**: (1) `staking_reserved += staker_share` **intero** — la dust d'accumulatore non esiste più (foundation capped ESATTAMENTE al 15% nominale); (2) `reward_debt` (unità) sostituito da **`paid_acc`** (snapshot dell'accumulatore, il `userRewardPerTokenPaid` di Synthetix) con `pending = ⌊staked × (acc − paid_acc) / P⌋` — **una sola floor sulla differenza**, sempre ≤ entitlement vero. Solvibilità per costruzione: ogni batch aggiunge `staker_share` sia a `reserved` sia al monte-entitlement; ogni settle paga ≤ entitlement maturato ⇒ `reserved ≥ Σ pending ≥ 0`, sempre. Gate esteso: il property test asserisce anche `reserved ≥ Σ pending ≥ 0` dopo ogni operazione (la sola uguaglianza a 4 bucket non intercettava il bug). Più 7 incoerenze chiuse: `CONSENSUS_VERSION` allineata, `ROUND_TOO_LATE` chiarito come rimosso, `SupplyOutOfRange` dichiarato unreachable, regola `SelfTransferNoop` scritta, posizione di `NonceExhausted` pinnata, testo legacy pre-age sostituito, typo changelog v0.8.4.

**Changelog v0.9.1 — chiusura matematica dei due P0 + formalizzazioni P1/P2**: (P0-A) nuovo bucket contabile **`staking_reserved`** in `global`: l'invariante monetario a quattro bucket è ora un'uguaglianza ESATTA per costruzione (il vecchio invariante con Σ pending era matematicamente falso per doppia floor — controesempio in §8); due strati di rounding distinti (dust d'accumulatore → foundation; residuo per-account → passività del protocollo, mai bruciato né attribuito); property test consensus-grade come gate. (P0-B) fairness riformulata come ciò che è dimostrabile: **blindness pre-beacon + receipt accountability + ordinamento deterministico + verificabilità dello state root** — il consenso NON vincola temporalmente la chiusura della raccolta al beacon (la vecchia §5.1 dichiarava A implementando B); la protezione per-blob è la ricevuta ottenuta prima della deadline (policy client, non consenso). (P1) `GENESIS_DRAND_ROUND` e mapping normativo `round(h) = G + h − 1` con **empty-block catch-up** (mai skip di round: il wall-clock non entra nel consenso, le finestre HTLC restano intere); profilo **POPCORN-TLOCK-AGE-V1**; tabelle normative dei discriminanti `RejectReason`/`FailReason`; `AddLiquidity` con `amount_actual`; sequenza exact-out esplicita; `unusable` come *derived evidence* con derivazione normativa; annex CONSENSUS-LOCK per le versioni esatte. (P2) root su liste vuote; ricevuta Borsh-canonica; ordinamento canonico dei feed Publish; encoding del singleton `global`; tassonomia errori beacon (`ROUND_TOO_LATE` RIMOSSO dal consenso: il ritardo è telemetria, non categoria del ledger); overflow per-caso incluso `NonceExhausted`; tie-break tx_id formale; scope del k-invariant; wording su node-key compromise.

**Changelog v0.9 — il consenso diventa normativo**: (1) nuova **§13 POPCORN-CONSENSUS**: ogni semantica che può influenzare lo state root è definita algoritmicamente con versione pinnata, `CONSENSUS_VERSION` esplicita e classificazione dei cambiamenti breaking/non-breaking — il determinismo è normativo, non emergente; (2) firma ed25519 = **`ed25519_dalek::verify_strict` a versione pinnata**, regola di consenso (una semantica scelta e congelata; ZIP-215 solo se mai servirà verifica batch); (3) **zero collezioni non ordinate nel commitment** (policy architetturale, non solo patch hashbrown ≥0.15.1) + test di canonicità obbligatorio; (4) **`TimelockProvider`** come unica interfaccia verso tlock/drand con **policy consensus-defined per beacon mancante/invalido**; (5) dichiarato: il timelock è fair ordering a orizzonte breve, MAI conservazione a lungo termine (`BLOB_ROUND_HORIZON`); (6) migrazione a alloy-primitives declassata a P3 post-freeze; (7) obbligo di **test vector consensus-grade** e reference executor indipendente; avanzamento dello stream XOF al rifiuto reso esplicito.

**Changelog v0.8.4 — protocol freeze audit**: (🔴1) formato blob = **age con recipient tlock** (`tlock_age` vendorizzato): la primitiva raw tlock cifra 16 byte, e l'ibrido chiave+AEAD che la spec descriveva a mano è esattamente ciò che il formato age standardizza — via il layer custom, interop Rust↔Go↔JS garantita dal formato condiviso, test vector cross-language gate di CI; (🔴2) superficie DoS della raccolta cieca congelata come **requisiti operativi fuori consenso** + benchmark worst-case obbligatorio pre-genesis; (🟠) semantica foundation con `total_staked == 0` riscritta senza ambiguità (15% nominale = 100% dell'emissione effettiva di quel batch); (🟠) regole di validazione AMM esplicitate; (🟡) `undecryptable` rinominato **`unusable`** (copre tlock/AEAD/decode falliti — un Borsh-fail è decifrabile ma inutilizzabile); (🟡) `collection_root` impegna l'**insieme** dei blob distinti, non la molteplicità delle submission; conteggio dipendenze corretto (10).

**Changelog v0.8.3 — fix contabili da review**: (1) fee prelevate in **fase unica** dopo l'ordinamento e prima dell'esecuzione — il `min()` sparisce, ogni tx paga sempre la fee piena, chiuso il bypass "svuoto il saldo con la prima tx e le altre girano gratis"; (2) §8 impone **intermedie U256** (S×acc sfora u128 nel caso dust-staker; i risultati rientrano in u128 per i bound di supply); (3) quattro pin: indice hashlock = cache derivabile fuori dallo state root; auto-settlement DOPO l'esecuzione (claim nello stesso batch vince, il publish diventa no-op); materializzazione pubkey solo su tx eseguite (mai su rejected); Borsh-fail = unusable. Note minori: griefing HtlcDuplicateHashlock, timestamp ricevuta non impegnato, manifest come insieme, Unstake a saldo zero.

**Changelog v0.8.2 — collection commitment (commit-then-decrypt)**: il nodo si impegna sull'insieme dei blob ricevuti PRIMA di poterli decifrare — `collection_root` nell'header, `blob_manifest` e lista `unusable` nel blocco, contabilità completa manifest → txs/rejected/unusable verificabile da chiunque col beacon pubblico. La censura selettiva post-decrypt diventa o auto-contraddizione firmata (ricevuta vs manifest) o menzogna pubblicamente falsificabile (unusable smentibile). Precedente: commit-then-decrypt di Shutter e accountability delle inclusion list Ethereum, adattati al nodo unico. Residuo dichiarato: rifiuto cieco all'ingresso (nessuna ricevuta emessa).

**Changelog v0.8.1 — auto-settlement HTLC via Publish**: alla chiusura del batch, ogni `Publish` con `data` di esattamente 32 byte il cui `sha256(data)` corrisponde all'hashlock di un HTLC aperto regola automaticamente quell'HTLC verso il recipient — il preimage diventa carrier-independent (chiunque può consegnarlo, il recipient può essere offline) e la censura del settlement richiede censura cieca di massa, auto-incriminante via ricevute §9.2. `HtlcClaim` resta come via diretta. Indice `hashlock → htlc_id` nello stato. Caso limite dichiarato: l'inclusione resta l'unico cancello (limite del modello, §1).

**Changelog v0.8 — HTLC (bridge user-side non-custodial)**: tre azioni `HtlcLock`/`HtlcClaim`/`HtlcRefund` per swap atomici cross-chain portati dagli utenti; hashlock **SHA-256** (lingua franca Bitcoin/Lightning/EVM/Solana — unico punto del protocollo non-blake3, per interoperabilità), preimage fisso 32 byte; refund invocabile da chiunque (garbage collection); vita massima dell'HTLC; tabella `htlcs` nello state root e invariante monetario esteso; note di sicurezza da letteratura (MAD-HTLC/bribery, stagger dei timeout, free option).

**Changelog v0.7.2 — fix di protocollo**: bootstrap account riparato (`signer_pubkey` nel `SignedTx`, modello P2PKH: la chiave si materializza alla prima firma — ed25519 non ha key recovery); prova di uniformità di `uniform(n)` inserita a commento (l'algoritmo era corretto); guard di liquidità su `Stake` (resta sempre una fee per uscire); `rejected_root` impegna anche il `RejectReason`; ricevuta di submit riqualificata come *evidence of receipt*; pinning d'interoperabilità drand/tlock con test vector cross-language; commitment del blocco esplicitato (si firma l'Header). Ledger verificabile, equità di ordinamento per costruzione (timelock + shuffle da beacon), DEX interno con meccanica Uniswap V2 estesa (multi-hop, fee tier, exact-out). Utenti: bot **e** persone (wallet Solana via firma messaggi).

**Changelog v0.7 — fair launch**: `GENESIS_SUPPLY = 0` (nessuna allocazione al creatore); quando `total_staked == 0` la quota staker **non viene emessa** (non nasce, né a foundation né bruciata) — la foundation è capped al 15% dell'emissione dal primo batch; supply massima ≈ 21,02 M; bootstrap via quota foundation dal blocco 1.

**Changelog v0.6 — dati utente e fee Solana-level**: nuova azione `Publish` (bacheca dati per oracoli portati dagli utenti — nessun effetto sullo stato, il dato vive nel blocco); `FEE_TX` allineata a Solana (5.000 unità = 0,000005 nativi); sovrapprezzo per byte sui publish oltre la soglia gratuita.

**Changelog v0.5.2 — chiusura contabile pre-implementazione**: staking dust contabilizzata (→ foundation); definizione congelata di `native_emitted`; momento dell'emissione fissato (chiusura batch, non spendibile nello stesso); pipeline di validazione statica ordinata con tie-break dei duplicati; identità di transazione dichiarata; cap effettivo in forma esatta; dicitura fee corretta.

**Changelog v0.5.1 — rigore contabile**: cap della supply definito come limite superiore (l'emissione effettiva la calcola il verificatore batch per batch); `pending` dichiarato passività contabile derivata; fee-solvency in validazione statica + fallback runtime; staking della foundation dichiarato esplicitamente legittimo.

**Changelog v0.5 — nuova economia e accesso**:
- **Rimossi faucet, inviti e `CreateAccount`**: account impliciti alla prima ricezione di fondi (modello Ethereum).
- **Foundation pool** al genesis con `GENESIS_SUPPLY` (account pubblico come ogni altro).
- **Emissione per batch con halving** (stile Bitcoin): supply totale finita in forma chiusa, split 85/15 staker/foundation.
- **Fee bruciate** (stile EIP-1559): niente più split 50/50.
- **Compatibilità wallet Solana** (Phantom/Solflare via `signMessage`): definito il flusso client e il dominio di firma.

**v0.4**: semantiche congelate (nonce+shuffle, failure, ID domain-separated, state root, header). **v0.3**: tlock+drand_core vendorizzati, redb, shuffle su blake3 XOF, beacon per round multi-endpoint. **v0.2**: multi-hop, fee tier, exact-out.

---

## 1. Modello di fiducia

- **Un solo nodo** (l'operatore). Nessun consenso distribuito, nessun P2P. Classificazione onesta: *single-operator deterministic/verifiable execution chain*.
- Fiducia sostituita da **verificabilità**: chiunque scarica la catena e ri-esegue lo stato da zero, inclusa **ogni emissione e ogni burn** (l'offerta monetaria è interamente ricostruibile dal replay).
- **Le quattro garanzie reali (v0.9.1)** — POPCORN non promette censorship resistance; promette, e può dimostrare: (1) **blindness pre-beacon**: il timelock impedisce all'operatore di conoscere il plaintext di qualunque blob prima che il beacon del round esista; (2) **ordinamento deterministico**: l'ordine di esecuzione è funzione del beacon, mai scelta dell'operatore; (3) **receipt accountability**: per ogni blob ricevutato, l'omissione dal manifest è una contraddizione tra due firme del nodo; (4) **verificabilità dello state root**: qualunque scorrettezza contabile o di esecuzione diverge nel replay. L'operatore non può riscrivere la storia, emettere fuori formula o alterare la supply senza divergenza di `state_root`.
- **Ciò che il consenso NON garantisce (dichiarato)**: il nodo non è matematicamente obbligato a chiudere la raccolta prima del beacon — può attendere il beacon, decifrare e poi selezionare cosa manifestare, producendo un blocco formalmente valido. La protezione contro questa finestra è per-blob: la **ricevuta ottenuta prima della deadline** (§9.2). POPCORN fornisce accountability forte per i blob per i quali il nodo ha emesso una ricevuta, **non una prova crittografica universale della ricezione di ogni pacchetto inviato**: è il limite scelto del modello single-operator, non un difetto nascosto.
- L'operatore **può**: censurare tx (rilevabile via ricevuta firmata §9.2, non impedibile) e dispone della chiave dell'account `FOUNDATION` — i cui movimenti sono però pubblici e tracciati come quelli di chiunque.
- **Chiavi separate (congelato)**: la **node key** (firma i blocchi, hot sul server) e la **foundation key** (custodisce valore, fredda, offline) sono chiavi ed25519 **distinte**, entrambe dichiarate nel genesis. La node key non controlla fondi; la foundation key non firma blocchi. Compromissione della node key (v0.9.1): non consente di spendere fondi, ma consente di produrre una **fork validamente firmata** — la divergenza è pubblicamente rilevabile, ma la selezione della storia canonica resta una proprietà del modello single-operator, non protetta da consenso distribuito (mitigazioni operative: mirror esterni timestampati, osservatori che archiviano gli header). Compromissione della foundation key: perdita dei fondi foundation, ledger integro.
- La vendita/distribuzione del token nativo contro asset esterni (SOL, fiat, …) avviene **off-chain ed è fuori protocollo**: la chain non custodisce asset di altre reti (nessun bridge).

---

## 2. Dipendenze

Tutto Rust, un solo binario, **zero codice nativo C/C++**.

### 2.1 Crate esterni (10 — serde e serde_json contati separatamente)

| Crate | Ruolo | Funzioni/tipi usati | Stato audit |
|---|---|---|---|
| `tokio` | Runtime async | `#[tokio::main]`, `tokio::spawn`, `tokio::time::interval` | Battle-tested |
| `axum` | HTTP/WebSocket server | `Router`, `routing::{get, post}`, `extract::{State, Json, Path}`, `WebSocketUpgrade` | Battle-tested |
| `ed25519-dalek` v2 | Firme account e blocchi | `SigningKey`, `VerifyingKey`, `Signature`, `Signer::sign`, `Verifier::verify` | Audit curve25519-dalek (2023) |
| `blake3` | Hashing + XOF | `blake3::hash`, `Hasher::{new, update, finalize, finalize_xof}`, `OutputReader::fill` | Impl. ufficiale degli autori |
| `borsh` | Serializzazione canonica | `BorshSerialize`, `BorshDeserialize`, `borsh::to_vec`, `borsh::from_slice` | Battle-tested (NEAR, Solana) |
| `age` | Formato del blob cifrato (usato via `tlock_age`) | cifratura/decifratura del payload nel formato age (STREAM ChaCha20-Poly1305 interno) | Formato pubblicamente specificato, implementazione battle-tested; interop = proprietà del formato |
| `sha2` | Solo hashlock HTLC (§7.6, interop cross-chain) | `Sha256::digest` | RustCrypto, famiglia coperta da audit NCC (2020) |
| `primitive-types` | U256 per AMM | `U256::{from, checked_mul, checked_div, integer_sqrt}` | Battle-tested (Parity) |
| `redb` | Storage (single-file, ACID, pure Rust) | `Database::create`, `TableDefinition`, `begin_write`, `begin_read`, `open_table`, `insert`, `get`, `commit` | Formato stabile, mantenuto, fuzzing continuo |
| `serde` + `serde_json` | Solo layer API (mai per hashing/stato) | derive `Serialize/Deserialize` | Battle-tested |

### 2.2 Crate vendorizzati nel workspace (2)

| Crate | Origine | Motivo |
|---|---|---|
| `tlock` | thibmeu/tlock-rs 0.0.5 (MIT, ~750 SLoC) | Upstream fermo dal 2024; protocollo congelato → fork interno pinnato, ispezionabile. **Non auditato**; schema peer-reviewed, interop con drand/tlock Go |
| `drand_core` | thibmeu/drand-rs 0.0.16 (MIT, ~1.500 SLoC) | Come sopra. Verifica BLS contro chain-info pinnata; unchained/G1 |
| `tlock_age` | thibmeu/tlock-rs (MIT) | Il formato del blob: age con recipient tlock. La primitiva raw `tlock` cifra 16 byte (la file key); `tlock_age` la usa dentro il formato age per payload arbitrari — è l'ibrido "chiave timelock + AEAD" standardizzato, interoperabile con drand/tlock Go e tlock-js |

Funzioni: `tlock::encrypt(&mut dst, src, &pubkey, round)`, `tlock::decrypt(&mut dst, src, &signature)`; `drand_core::HttpClient`, `chain_info()`, `get(round)`, `ChainInfo::public_key()`, `Beacon::{signature(), round()}`.

**Regole ferree**: (a) tutto ciò che entra in un hash o nello stato passa da Borsh, mai JSON; (b) il blob è formato age con recipient tlock (§3); (c) **zero collezioni non ordinate nel commitment (POLICY ARCHITETTURALE, v0.9)**: nello stato serializzato e in ogni struttura che tocca un root sono ammessi solo `BTreeMap`/`BTreeSet`/`Vec` a ordine esplicito/array/primitive — `HashMap`/`HashSet` e qualsiasi iterazione non ordinata sono VIETATI *indipendentemente* dalla versione di borsh/hashbrown (riferimento: RUSTSEC-2024-0402, encoding non canonico → consensus split; la patch hashbrown ≥0.15.1 è dovuta ma non sostituisce la policy). CI: gate che fallisce se una struttura consensus introduce una collezione vietata + **test di canonicità**: stesso stato logico, ordini di inserimento diversi → byte identici → state_root identico.

**Pinning d'interoperabilità (congelato al genesis)**: `DRAND_SCHEME = bls-unchained-g1-rfc9380`; `DRAND_CHAIN_HASH = 52db9ba70e0cc0f6eaf7803dd07447a1f5477735fd3f661792ba94600c84e971` (quicknet); commit esatto dei fork vendorizzati di `tlock`/`tlock_age`/`drand_core` registrato nel genesis (profilo: primitiva raw tlock a 16 byte NON modificata; payload via formato age); suite di **test vector cross-language** nel repo come gate di CI: encrypt in Rust/tlock-js/tlock-Go, decrypt in ciascuna delle altre, byte-identici. È la condizione perché la promessa "verifica riproducibile in qualsiasi linguaggio" valga anche per la cifratura, non solo per il replay.

---

## 3. Primitive crittografiche e client

- **Identità account**: ed25519. `AccountId = blake3(verifying_key)` (32 byte). Stessa curva di Solana: **una keypair Phantom/Solflare è un'identità valida**.
- **Dominio di firma** (congelato): la firma è ed25519 su `blake3(SIGN_DOMAIN || borsh(payload))` con `SIGN_DOMAIN = "popcorn-v1"` (ASCII, 10 byte). Impedisce il riuso cross-chain di firme prodotte da wallet Solana e viceversa.
- **Semantica di verifica (REGOLA DI CONSENSO, v0.9)**: `signature_valid = ed25519_dalek::<VERSIONE_PINNATA>::verify_strict(...)` — non "qualunque Ed25519 standard". RFC 8032 è sotto-specificato e le implementazioni divergono su firme borderline; il replay dei verificatori deve accettare/rifiutare ESATTAMENTE le stesse firme del nodo, quindi la semantica è definita dall'algoritmo, versione inclusa. `verify_strict` (rifiuta punti di piccolo ordine e s non canonici) è la scelta congelata per il modello single-signer; NON è "intrinsecamente migliore" di ZIP-215 — è UNA semantica, scelta e congelata. Se mai servisse verifica batch, la migrazione a ZIP-215 (`ed25519-zebra`) sarebbe un cambio di consenso dichiarato. Test vector obbligatori con firme borderline (torsion, R/A non canonici) che nodo e verificatore devono trattare identicamente.
- **Flusso client umano (wallet Solana)**: frontend web → Wallet Adapter `signMessage(blake3(SIGN_DOMAIN || borsh(payload)))` → cifratura con **tlock-js** verso il round target → `POST /tx`. Nessun RPC Solana emulato: il wallet firma, il nostro client parla con la nostra API.
- **Flusso client bot**: keypair ed25519 su file + client HTTP; cifratura con tlock (Rust), tlock-js (JS/TS) o drand/tlock (Go) — interoperabili.
- **Beacon**: drand **quicknet** (unchained, 3 s). Chain-hash pinnato in genesis. Verifica BLS di ogni beacon contro la `ChainInfo` pinnata. Fetch **sempre per round** (`get(R)`), mai `/latest`; fallback in ordine su `DRAND_REMOTES`.
- **Policy beacon (REGOLA DI CONSENSO, v0.9)** — esiti enumerati, ciascuno con UNA conseguenza deterministica; nulla è lasciato al runtime:
  - `ROUND_AVAILABLE` (firma BLS verificata) → si produce il blocco `R`;
  - `ROUND_NOT_AVAILABLE` (nessun remote risponde) → retry con backoff, **la chain ritarda**: nessun blocco per `R` finché il beacon non arriva; mai skip, mai randomness di fallback;
  - `ROUND_INVALID` (firma BLS non verifica) → come NOT_AVAILABLE su quel remote; se TUTTI i remote danno firma invalida per lo stesso round, il nodo si FERMA (halt esplicito, mai un blocco con beacon non verificato);
  - il ritardo del beacon NON è una categoria di consenso (v0.9.1): è liveness/telemetria — per il ledger esiste solo "beacon valido per il round atteso".
- **Tassonomia degli errori di fetch (v0.9.1)**: `FETCH_FAILURE` (HTTP/timeout), `MALFORMED_RESPONSE`, `WRONG_ROUND`, `WRONG_CHAIN` sono errori del *remote* → prossimo remote / retry (equivalgono a NOT_AVAILABLE); solo `INVALID_SIGNATURE` **da tutte le fonti fidate** per lo stesso round porta all'halt.
- **Mapping round→blocco (REGOLA DI CONSENSO, v0.9.1)**: `round(h) = GENESIS_DRAND_ROUND + h − 1` per ogni `h ≥ 1`, con `GENESIS_DRAND_ROUND` impresso nel genesis. **Mai skip di round**: dopo un downtime il nodo recupera producendo i blocchi mancanti in sequenza — beacon del round atteso → collection vuota → blocco vuoto (con la sua emissione) → round successivo. Nessuna regola speciale di catch-up: il caso "gap" non esiste, esiste solo la sequenza. Il wall-clock non entra mai nel consenso; le finestre HTLC restano intere per costruzione (un expiry dentro il downtime viene attraversato, mai scavalcato).
- **`TimelockProvider` (v0.9)**: tutta la logica tlock/drand vive dietro un unico trait (`encrypt/decrypt/chain_hash/round_for_time/get_beacon`); la state machine non sa quale implementazione c'è sotto. La dipendenza timelock è **sostituibile per costruzione** — tlock-rs oggi, un'implementazione nativa su BLS auditato domani — non parte inseparabile dell'architettura.
- **Orizzonte del timelock (congelato v0.9)**: il nodo accetta blob solo per round entro `BLOB_ROUND_HORIZON` dal round corrente. **Il timelock di POPCORN è fair ordering a orizzonte di secondi/minuti, MAI conservazione a lungo termine**: un eventuale sunset di drand (precedente: fastnet, key material distrutto) impatterebbe l'availability della chain, non la decifrabilità di ciphertext lontani — che per costruzione non esistono.
- **Profilo POPCORN-TLOCK-AGE-V1 (REGOLA DI CONSENSO, v0.9.1)** — age è un formato estensibile: POPCORN non accetta "qualunque ciphertext age valido" ma solo questo profilo: formato age v1 binario, **armor vietato**; **esattamente una** recipient stanza, di tipo tlock verso il round target; encoding canonico (base64 canonico, LF only); header ≤ 1 KiB; payload STREAM v1; qualsiasi deviazione ⇒ `unusable`. L'**acceptance policy è normativa**: nodo e verificatori devono accettare/rifiutare gli stessi byte — i test vector cross-language coprono anche i casi di rifiuto, non solo il round-trip.
- **Cifratura tx (congelato v0.8.4)**: il blob è il payload Borsh cifrato nel **formato age con recipient tlock verso il round `R`** (`tlock_age`): age genera la file key, la protegge con la primitiva tlock (16 byte, com'è nativa), e cifra il payload con STREAM/ChaCha20-Poly1305 — l'ibrido è il formato, non codice nostro. Interop client: **drand/tlock (Go), tlock-js, tlock_age (Rust)** producono e leggono lo stesso ciphertext; la suite di test vector cross-language (§2.2) è gate di CI.
- **Shuffle deterministico** (fissato dalla spec, non da una libreria):

```
seed_stream = blake3::Hasher::new()
                .update(drand_signature_R)      // bytes grezzi della firma BLS
                .update(LE64(height))
                .finalize_xof()

next_u64():  8 byte dallo stream (OutputReader::fill), little-endian

uniform(n):  limit = u64::MAX - (u64::MAX % n)   // = ⌊(2⁶⁴−1)/n⌋·n → multiplo esatto di n
             loop { x = next_u64(); if x < limit { return x % n } }
             // al rifiuto: gli 8 byte SUCCESSIVI dallo stream (lo stream avanza
             // sempre, mai riletture) — parte della definizione di consenso
// PROVA DI UNIFORMITÀ: l'insieme accettato è [0, limit), di cardinalità limit,
// multiplo esatto di n per costruzione → x % n è esattamente uniforme su [0, n).
// (Per n potenza di 2 si rigettano n valori in coda invece di 0: spreco
// trascurabile, uniformità intatta.)

// Fisher-Yates sulla lista tx valide ordinate per tx_id crescente (bytes lessicografici)
for i in (1..len).rev():
    j = uniform(i + 1)
    swap(txs[i], txs[j])
```

---

## 4. Identificatori e formati dati (Borsh)

### 4.1 Identificatori — domain separation (congelato)

Tutti gli ID sono `[u8; 32]`. Il primo byte del preimage è un **tag di dominio**: nessuna collisione tra namespace.

```
NATIVE_TOKEN  = [0x00; 32]                                        // costante
TokenId       = blake3(0x01 || creator: AccountId || LE64(payload.nonce))
LpTokenId     = blake3(0x02 || PairId)
PairId        = blake3(0x03 || token0 || token1 || LE16(fee_bps))  // token0 < token1 (lessicografico)
tx_id         = blake3(borsh(SignedTx))
HtlcId        = blake3(0x04 || sender: AccountId || LE64(payload.nonce))
FOUNDATION    = AccountId della chiave foundation, dichiarato nel genesis
```

**Identità di transazione (congelato)**: `tx_id` copre il `SignedTx` completo, **firma inclusa** — è l'hash di ciò che viene effettivamente trasmesso ed eseguito. Poiché un firmatario può produrre firme diverse per lo stesso payload, la stessa intenzione può generare più tx_id: è il dedup per nonce (§5.2, passo 6) a garantire che ne venga eseguita al più una.

**Invariante di identità (congelato)**: per ogni account con `pubkey == Some(pk)`: `AccountId == blake3(pk)`; la pubkey si materializza alla prima tx firmata inclusa (§4.2) e non è mai riassegnabile. La coerenza è garantita per costruzione: il nodo deriva **sempre** `signer = blake3(signer_pubkey)` dal campo della tx, mai da una lookup inversa.

**Limiti di dimensione (congelato)**: `MAX_BLOB_SIZE` è un limite **wire-level** sull'intero blob cifrato (formato POPCORN-TLOCK-AGE-V1, §3) accettato da `POST /tx`; `MAX_PUBLISH_SIZE` è un limite sul campo in chiaro `Action::Publish.data`. Sono indipendenti: un `Publish` da 512 B in chiaro deve comunque stare, cifrato e con overhead, nei 2 KiB wire.

Gli **LP token sono token di prima classe**: vivono in `balances` sotto `LpTokenId` (quindi `Transfer` funziona su di loro senza codice dedicato). `token0`/`token1` in `PairId` possono essere `NATIVE_TOKEN` o `TokenId`, **mai** `LpTokenId` (niente pool di LP: vietato in validazione).

### 4.2 Account impliciti e materializzazione della chiave (congelato)

Non esiste un'azione di creazione account. Un record `Account` nasce automaticamente (nonce 0, saldi vuoti, `pubkey = None`) la **prima volta che riceve fondi**: `Transfer` in ingresso, accredito LP, payout staking, output di swap. Il mittente indica solo `to: AccountId` — non deve conoscere la chiave del destinatario.

**Materializzazione**: ed25519 non ha key recovery, quindi la verifying key deve viaggiare esplicitamente — sta nel campo `signer_pubkey` di ogni `SignedTx`. Alla **prima tx firmata dall'account eseguita** (`Ok` o `Failed`, cioè presente in `txs`), `account.pubkey` passa da `None` a `Some(signer_pubkey)`, per sempre. Le `rejected` **non** materializzano: non deve essere possibile fissare la chiave senza pagare la fee. È il modello P2PKH di Bitcoin: si paga a un hash, la chiave si rivela al primo spend.

Un account senza saldo nativo non può transare (non copre `FEE_TX`): il bootstrap di un nuovo utente è ricevere nativo da qualcuno (foundation, un altro utente, un exchange interno).

### 4.3 Strutture

```rust
type AccountId = [u8; 32];
type Amount    = u128;       // 9 decimali per tutti i token

struct Account {
    pubkey: Option<[u8; 32]>,         // None finché l'account non firma la sua prima tx (§4.2)
    nonce: u64,                       // ultima nonce ESEGUITA (parte da 0)
    balances: BTreeMap<[u8;32], Amount>,   // NATIVE_TOKEN | TokenId | LpTokenId
    staked: Amount,
    paid_acc: u128,   // snapshot di acc_per_stake all'ultimo settle (Synthetix userRewardPerTokenPaid) — v0.9.2
}

struct Token {
    id: [u8; 32],
    creator: AccountId,
    name: [u8; 16],                   // ASCII stampabile 0x20–0x7E, zero-padded a destra
    total_supply: Amount,             // 1 ..= MAX_SUPPLY, immutabile
}

struct Pair {
    id: [u8; 32],
    token0: [u8; 32],
    token1: [u8; 32],
    fee_bps: u16,                     // ∈ FEE_TIERS
    reserve0: Amount,
    reserve1: Amount,
    lp_supply: Amount,
}

struct SignedTx {
    payload: TxPayload,
    signer_pubkey: [u8; 32],          // verifying key ed25519; signer = blake3(signer_pubkey)
    signature: [u8; 64],              // ed25519 su blake3(SIGN_DOMAIN || borsh(payload))
}

struct TxPayload {
    nonce: u64,
    target_round: u64,
    action: Action,
}

enum Action {
    Transfer      { token: [u8;32], to: AccountId, amount: Amount },
    CreateToken   { name: [u8;16], supply: Amount },
    CreatePair    { token_a: [u8;32], token_b: [u8;32], fee_bps: u16 },
    AddLiquidity  { pair: [u8;32], amount0_desired: Amount, amount1_desired: Amount,
                    amount0_min: Amount, amount1_min: Amount },
    RemoveLiquidity { pair: [u8;32], lp_amount: Amount,
                    amount0_min: Amount, amount1_min: Amount },
    SwapExactIn   { path: Vec<[u8;32]>, token_in: [u8;32],
                    amount_in: Amount, min_amount_out: Amount },
    SwapExactOut  { path: Vec<[u8;32]>, token_in: [u8;32],
                    amount_out: Amount, max_amount_in: Amount },
    Publish       { topic: [u8;32], data: Vec<u8> },   // §7.5 — nessun effetto sullo stato
    HtlcLock      { to: AccountId, token: [u8;32], amount: Amount,
                    hashlock: [u8;32], expiry_round: u64 },        // §7.6 — sha256(preimage)
    HtlcClaim     { htlc_id: [u8;32], preimage: [u8;32] },         // §7.6
    HtlcRefund    { htlc_id: [u8;32] },                            // §7.6 — invocabile da chiunque
    Stake         { amount: Amount },
    Unstake       { amount: Amount },
    ClaimRewards  {},
}

enum ExecStatus { Ok, Failed(FailReason) }     // allineato 1:1 a Block.txs

struct Header {
    height: u64,
    prev_hash: [u8; 32],              // block_hash del blocco precedente; genesis: [0;32]
    drand_round: u64,
    drand_sig_hash: [u8; 32],         // blake3(drand_signature)
    collection_root: [u8; 32],        // blake3(concat(blake3(blob))) ordinati lessicografici — §5.1
    txs_root: [u8; 32],               // blake3(concat(tx_id) nell'ORDINE DI ESECUZIONE)
    rejected_root: [u8; 32],          // blake3(concat(tx_id || borsh(RejectReason))), coppie ordinate per tx_id
    results_root: [u8; 32],           // blake3(borsh(Vec<ExecStatus>))
    state_root: [u8; 32],             // §5.4
}

struct Block {
    header: Header,
    drand_signature: Vec<u8>,         // firma BLS completa (per verifica e tlock replay)
    blob_manifest: Vec<[u8;32]>,      // blake3 di OGNI blob ricevuto per R, ordine lessicografico
    unusable: Vec<[u8;32]>,      // ⊆ manifest: tlock O age/AEAD O decodifica Borsh falliti
                                      // (un Borsh-fail è decifrabile ma inutilizzabile: da qui il nome)
    txs: Vec<SignedTx>,               // ordine di esecuzione
    results: Vec<ExecStatus>,
    rejected: Vec<([u8;32], RejectReason)>,   // tx_id, ordinati lessicograficamente
    node_signature: [u8; 64],         // ed25519 su block_hash
}

// Root su liste vuote (congelato v0.9.1): txs_root, rejected_root e
// collection_root su lista vuota = blake3 dell'input vuoto; results_root
// su lista vuota = blake3(borsh(Vec::<ExecStatus>::new())). Nessun caso speciale.
// block_hash     := blake3(borsh(Header))
// node_signature := Ed25519(node_key, block_hash)
// L'oggetto firmato/impegnato è l'HEADER; Block è un contenitore i cui campi
// (txs, results, rejected, drand_signature) sono vincolati dai root nell'Header.
```

**Semantica del path**: sequenza di `PairId`. Il nodo parte da `token_in`; per ogni pair deriva il token di uscita (l'altro lato). Validazione fallisce se una pair non contiene il token corrente. `1 <= path.len() <= MAX_PATH_LEN`.

**Storage (redb, single file)**: tabelle `blocks` (`u64 → borsh(Block)`, append-only — **fonte di verità**), `state` (cache ricostruibile per replay), `meta` (head, chain-info drand, pubkey nodo, pubkey foundation). Un `WriteTransaction` per batch: blocco + delta stato atomici.

**Genesis (blocco 0)**: nessuna tx, **nessuna allocazione** (fair launch): stato iniziale vuoto; `state_root` calcolato sullo stato vuoto; parametri e chain-info drand impressi in `meta`. I primi nativi nascono con l'emissione alla chiusura del blocco 1 (quota foundation).

---

## 5. Ciclo di vita del batch (semantiche congelate)

Un batch per round drand (3 s). Il batch `R` esegue le tx cifrate verso il round `R`.

### 5.1 Fasi
1. **Raccolta** (fino a `T(R) − ε`): `POST /tx` con `{blob, target_round: R}`; il nodo accoda alla cieca e firma la ricevuta (§9.2).
1b. **Commitment della raccolta (riformulato v0.9.1)**: il nodo congela il `blob_manifest` (blake3 di ogni blob ricevuto, ordine lessicografico) e ne calcola il `collection_root`. **Il consenso non vincola temporalmente questa chiusura al beacon**: un nodo può attendere il beacon, decifrare e poi manifestare — il blocco resta formalmente valido. Le garanzie sono: (a) ogni blob **ricevutato** deve comparire nel manifest (contraddizione firmata altrimenti, §9.2); (b) ogni voce del manifest deve risolversi nella contabilità completa (fase 3). Pratica operativa raccomandata (non consenso): pubblicare il root su `WS /stream` prima di `T(R)` per dare a osservatori terzi un timestamp del commit-then-decrypt.
2. **Beacon**: `get(R)` con fallback multi-endpoint; verifica BLS.
3. **Decifratura**: blob decifrato col beacon del round secondo il profilo POPCORN-TLOCK-AGE-V1 (§3). **Contabilità completa (congelata)**: ogni hash del manifest DEVE risolversi in esattamente uno tra — una tx in `txs`, una voce in `rejected`, o la lista `unusable`. Ogni claim di indecifrabilità è **falsificabile da chiunque**: blob + beacon pubblico → decifratura riproducibile. Il nodo serve i blob manifestati via `GET /blob/{hash}` e li include nel mirror. **Derivazione normativa di `unusable` (v0.9.1)**: `unusable := manifest ∖ {blob dei SignedTx in txs/rejected}` — il campo `unusable` del blocco è **derived evidence**, non input di consenso: due verificatori con manifest, blob (dal mirror) e beacon DEVONO derivare lo stesso insieme applicando il profilo POPCORN-TLOCK-AGE-V1 e la decodifica Borsh; un campo `unusable` incoerente con la derivazione è un blocco scorretto. **Pin (v0.8.3)**: il fallimento di decodifica Borsh del `SignedTx` decifrato conta come `unusable` (hash del blob), mai come `rejected` — una `rejected` impegna un `tx_id`, che per un blob non decodificabile non esiste. Il `collection_root` impegna l'**insieme dei blob distinti ricevuti, NON la molteplicità delle submission**: lo stesso blob sottomesso dieci volte (dieci ricevute) è una voce del manifest — un manifest multiset è una implementazione errata. Il dedup nonce gestisce comunque il duplicato logico.
4. **Validazione statica** (§5.2) → `valid` + `rejected`.
5. **Ordinamento** (§5.3): shuffle + normalizzazione nonce.
5b. **Prelievo fee (congelato v0.8.3)**: per ogni tx valida, burn di `tx_fee(tx)` dal signer, in fase unica — la solvency statica (passo 9, saldo pre-batch) garantisce capienza, il prelievo non può fallire. L'esecuzione parte a fee già bruciate.
6. **Esecuzione sequenziale** (§5.2).
7. **Chiusura**: emissione (§7.2), burn delle fee (§7.4), state root (§5.4), header, firma, commit atomico redb, push WS, mirror esterno.

Tx in ritardo per `R`: scartate (il client ricifra verso un round futuro).

### 5.2 Validazione e failure (congelato)

**Validazione statica — pipeline ordinata (congelata).** I passi si applicano in quest'ordine; una tx scartata a un passo non partecipa ai successivi:
1. decodifica Borsh del `SignedTx`;
2. firma ed25519 di `signer_pubkey` valida su `blake3(SIGN_DOMAIN || borsh(payload))`; si deriva `signer = blake3(signer_pubkey)`;
3. `target_round == R`;
4. l'account `signer` esiste; se `account.pubkey == Some(pk)`, richiesto `pk == signer_pubkey` (se `None`, la materializzazione avviene all'inclusione, §4.2); **se `account.nonce == u64::MAX` → `rejected: NonceExhausted`** (account terminale, valutato QUI — prima di dedup e contiguità; sotto-ordine del passo 4, v0.9.3: esistenza → coerenza pubkey → nonce esaurita, quindi a parità di condizioni vince `PubkeyMismatch`);
5. campi nei range: `fee_bps ∈ FEE_TIERS`, `supply ∈ 1..=MAX_SUPPLY`, `1 <= path.len() <= MAX_PATH_LEN`, nome ASCII stampabile, `data.len() <= MAX_PUBLISH_SIZE`; **`amount > 0` per `Transfer`, `Stake`, `Unstake`** (v0.9.3 → `FieldOutOfRange`); per `HtlcLock`: `amount > 0` e `R < expiry_round <= R + HTLC_MAX_LIFETIME_ROUNDS`;
6. **dedup nonce**: a parità di `(signer, nonce)` resta la tx con `tx_id` lessicograficamente minore, le altre → `rejected`; tie-break formale (v0.9.1): `tx_id` uguali ⇒ `SignedTx` byte-identici ⇒ stessa transazione, che collassa in una sola voce;
7. **contiguità**: le nonce dell'account devono formare una sequenza contigua da `account.nonce + 1`; le tx oltre il primo buco → `rejected`;
8. **budget**: al più `MAX_TX_PER_ACCOUNT_PER_BATCH` tx per account, tenute in ordine di nonce crescente (eccedenti → `rejected`);
9. **fee-solvency**: saldo nativo **pre-batch** ≥ `Σ tx_fee(tx)` delle tx dell'account sopravvissute ai passi 1–8; se insolvente, si scartano le tx **dalla nonce più alta in giù** finché la condizione vale (scartate → `rejected: FeeInsolvent`).

Nota: un account finanziato nello stesso batch può transare solo dal batch successivo.

**Regola Transfer (v0.9.2)**: `Transfer` con `to == signer` ⇒ `Failed(SelfTransferNoop)` — nessun trasferimento a sé stessi (pagherebbe fee per un no-op ambiguo in contabilità).

**Fee canonica (congelata)**:
```
tx_fee(tx) = FEE_TX + PUBLISH_BYTE_FEE × max(0, len(data) − PUBLISH_FREE_BYTES)   se action è Publish
tx_fee(tx) = FEE_TX                                                                altrimenti
```
È l'**unica** definizione di fee, usata ovunque: solvency (passo 9), burn su `Ok`, burn su `Failed`.

**`rejected`** (fallita la statica): non eseguita, **zero fee, nonce intatto**; nel blocco come `(tx_id, RejectReason)`.

**Fee e esecuzione (congelato v0.8.3)**: le fee di TUTTE le tx valide sono bruciate nella fase 5b, prima che qualsiasi azione esegua — ogni tx paga sempre la fee piena (`Ok` e `Failed`), nessuna azione può spendere i fondi destinati alle fee successive, e il `min()` non esiste più. Poi, per ogni tx nell'ordine: (1) snapshot dello stato; (2) esecuzione dell'azione. Il rollback di `failed` ripristina lo snapshot: la fee, bruciata in 5b, è fuori dal rollback per costruzione e mai restituita.

**`failed`** (fallita a runtime — `min_amount_out` violato, saldo insufficiente, pool inesistente al momento dell'esecuzione, overflow): rollback allo snapshot, **nonce consumato** (`account.nonce += 1`), `ExecStatus::Failed(reason)` in `results`.

**`Ok`**: effetti applicati, nonce consumato.

### 5.3 Nonce + shuffle (congelato)

Lo shuffle globale (§3) assegna le **posizioni**. Poi, **normalizzazione per account**: per ogni account con più tx nel batch, siano `P = {p1 < p2 < …}` le posizioni delle sue tx dopo lo shuffle; le sue tx vengono riassegnate a `P` in **ordine di nonce crescente**. Le tx di account diversi non si muovono.

Proprietà: deterministico; distribuzione delle posizioni uniforme; una sequenza contigua non fallisce per disordine interno; un `failed` intermedio non blocca le successive dello stesso account (nonce comunque consumata).

### 5.4 State root canonico (congelato)

```
h = blake3::Hasher::new()
per tabella in [0x01 accounts, 0x02 tokens, 0x03 pairs, 0x04 htlcs, 0x05 global]:
    h.update([tag_tabella])
    per (k, v) nella tabella, con k in ordine lessicografico dei bytes borsh(k):
        bk = borsh(k); bv = borsh(v)
        h.update(LE32(len(bk))); h.update(bk)
        h.update(LE32(len(bv))); h.update(bv)
state_root = h.finalize()
```

`global` (ordine di campo congelato, v0.9.1): `height: u64`, `total_staked: Amount`, `acc_per_stake: u128`, `staking_reserved: Amount`, `native_emitted: Amount`, `native_burned: Amount`, `account_count: u64`.

**Encoding del singleton `global` (congelato v0.9.1)**: la tabella 0x05 contiene esattamente una coppia a chiave vuota: `h.update([0x05]); h.update(LE32(0)); h.update(LE32(len(borsh(global)))); h.update(borsh(global))` — nessuna inferenza richiesta a un implementatore non-Rust.

**Invariante monetario a quattro bucket (RISCRITTO v0.9.1 — uguaglianza ESATTA per costruzione)**:
`Σ balances[NATIVE] + Σ staked + Σ htlcs[token==NATIVE].amount + staking_reserved = GENESIS_SUPPLY + native_emitted − native_burned`

Ogni unità nativa vive in **esattamente uno** di quattro posti: saldo liquido, stake, escrow HTLC, o passività di staking (`staking_reserved`). Nessuna unità "vive dentro una formula". Il vecchio invariante con `Σ pending` era **matematicamente falso** (doppia floor su basi diverse: `Σ⌊xᵢ⌋ ≤ ⌊Σxᵢ⌋` — controesempio in §8) ed è sostituito. `pending(a) = ⌊staked × (acc_per_stake − paid_acc) / PRECISION⌋` (v0.9.2, Synthetix letterale) è la formula derivata che determina quanto un settle *trasferisce* da `staking_reserved` al saldo — `staking_reserved ≥ Σ pending(a) ≥ 0` vale **per costruzione** (dimostrazione in §8), e la differenza è il rounding residue: passività del protocollo non attribuibile senza O(N), mai bruciata né girata a foundation. `native_emitted` significa esattamente: unità nominali create dal protocollo; `staking_reserved`: unità create come quota staking e non ancora trasferite a saldo.

---

## 6. Matematica AMM (Uniswap V2 generalizzata al fee tier)

Aritmetica intermedia in `U256`, risultato in `u128` con check. Divisioni: floor. Nessun float. `fee_num = 10_000 - fee_bps`.

**Exact-in (hop):**
```
amount_in_with_fee = amount_in * fee_num
amount_out = (amount_in_with_fee * reserve_out)
           / (reserve_in * 10_000 + amount_in_with_fee)
```

**Exact-out (hop):**
```
amount_in = (reserve_in * amount_out * 10_000)
          / ((reserve_out - amount_out) * fee_num) + 1
require(amount_out < reserve_out)   // violazione ⇒ Failed(SlippageExceeded) (v0.9.3)
```

**Multi-hop**: exact-in in avanti hop per hop, `require(out_finale >= min_amount_out)`. **Exact-out (sequenza esplicita, v0.9.1)**: con `path = P1..Pk` e output finale desiderato `X`: `in_k = exact_out(Pk, X)`, `in_{k−1} = exact_out(P_{k−1}, in_k)`, …, `in_1 = exact_out(P1, in_2)`; `require(in_1 <= max_amount_in)`; poi esecuzione **in avanti** con esattamente gli importi della passata a ritroso (`P1: in_1 → in_2`, …, `Pk: in_k → X`), mai ricalcolati. Ogni hop aggiorna le riserve della sua pair; la fee dell'hop resta agli LP di quel pool. La tx multi-hop è atomica (§5.2).

**Liquidità (lifecycle congelato):**
```
genesi (reserve0 == 0 && reserve1 == 0 && lp_supply == 0):
            liquidity = integer_sqrt(amount0 * amount1) - MINIMUM_LIQUIDITY
            require(liquidity > 0)
            // MINIMUM_LIQUIDITY accreditata a lp_supply ma a nessun account (bruciata)

ri-genesi (reserve0 == 0 && reserve1 == 0 && lp_supply == MINIMUM_LIQUIDITY):
            // pair completamente svuotata: riparte con la formula genesi
            liquidity = integer_sqrt(amount0 * amount1) - MINIMUM_LIQUIDITY
            require(liquidity > 0)
            // le MINIMUM_LIQUIDITY già bruciate restano le uniche bruciate
            // reserve entrambe 0 con lp_supply > MINIMUM_LIQUIDITY → Failed
            // (guard: LP residue su riserve nulle ruberebbero quota ai nuovi depositanti;
            //  vanno prima bruciate con RemoveLiquidity a resa nulla, poi la pair riparte)

successiva: liquidity = min(amount0 * lp_supply / reserve0,
                            amount1 * lp_supply / reserve1)
            require(liquidity > 0)

rimozione:  amount_i = lp_amount * reserve_i / lp_supply
            require(amount_i >= amount_i_min)
```

**Zero-output vietato (congelato)**: ogni hop di swap richiede `amount_out >= 1`; in caso contrario la tx è `Failed` (niente swap a resa nulla che pagano solo fee per muovere dust).

**Regole di validazione AMM esplicite (congelate v0.8.4)** — runtime, esito `Failed` se violate:
- `CreatePair`: `token_a != token_b`; entrambi esistenti (`NATIVE` o record `Token`); nessuno dei due è un `LpTokenId`; `fee_bps ∈ FEE_TIERS` (già statica); `PairId` non esistente.
- `AddLiquidity` (formalizzata v0.9.1): pair esistente; `amount0_desired > 0` e `amount1_desired > 0`. Per pool con riserve non nulle gli importi EFFETTIVI sono calcolati così (Router02):
  `a1_opt = ⌊amount0_desired × reserve1 / reserve0⌋`; se `a1_opt ≤ amount1_desired` → `(actual0, actual1) = (amount0_desired, a1_opt)`; altrimenti `a0_opt = ⌊amount1_desired × reserve0 / reserve1⌋` e `(actual0, actual1) = (a0_opt, amount1_desired)`.
  `require(actual0 ≥ amount0_min && actual1 ≥ amount1_min)`; si addebitano **solo gli actual** (l'eccesso `desired − actual` non viene MAI toccato); `reserve += actual`; mint sulla formula di liquidità con gli actual. Nei rami genesi/ri-genesi gli actual coincidono coi desired. Addebitare i desired è un'implementazione errata.
- `RemoveLiquidity`: pair esistente; `lp_amount > 0`; `lp_amount ≤` saldo LP del signer.
- `Swap*`: path non vuoto e `≤ MAX_PATH_LEN` (già statica); `token_in` appartiene alla prima pair; ogni hop esiste e contiene il token corrente; `amount_in > 0` / `amount_out > 0`.

Invariante `k_after ≥ k_before` (scope v0.9.1): vale **solo per gli hop di swap riusciti** — NON si applica ad `AddLiquidity`/`RemoveLiquidity`, che cambiano k per definizione. Vietati pool con LP token come lato (§4.1). `u128 × u128 < U256::MAX` sempre: niente overflow U256.

---

## 7. Economia (v0.5 — congelata)

### 7.1 Fair launch e foundation
- **Nessuna allocazione al genesis** (`GENESIS_SUPPLY = 0`): nessun premine, nessuna vendita primaria, nessun faucet, nessun invito. Tutti i nativi che esisteranno nascono dall'emissione (§7.2).
- `FOUNDATION` è un account normale (chiave dell'operatore, movimenti pubblici) che riceve la quota foundation dell'emissione. È il **bootstrap** dell'economia: i primi token in circolazione sono la sua quota dal blocco 1, che distribuisce via grant, pagamenti o liquidità nei pool perché altri possano transare e stakare.
- **La foundation può stakare** i propri fondi e percepire la quota staker come chiunque: scelta deliberata, coerente con "account normale" — nessuna regola speciale nel codice, e la concentrazione risultante è pubblica e leggibile on-chain da chiunque.
- Il mercato secondario del nativo (utenti che scambiano tra loro contro asset esterni) è off-chain e fuori protocollo.

### 7.2 Emissione per batch con halving (unica fonte di nuova supply)
```
emission_index    = height − 1                     // 0-based: blocco 1 → indice 0
EMISSION(height)  = EMISSION_0 >> (emission_index / HALVING_INTERVAL)
// così ogni epoca contiene ESATTAMENTE HALVING_INTERVAL batch
// (blocchi 1..=10_512_000 → epoca 0; dal 10_512_001 → epoca 1)

staker_share     = EMISSION(height) * EMISSION_STAKER_BPS / 10_000
foundation_share = EMISSION(height) - staker_share

se total_staked == 0:
    // la quota staker NON viene emessa: non nasce (né a foundation, né bruciata)
    native_emitted += foundation_share
altrimenti:
    native_emitted += EMISSION(height)
```
- `staker_share` entra INTERO nell'accumulatore/riserva staking (§8, v0.9.2 — nessuna dust: la floor vive solo lato utente); `foundation_share` accreditata a `FOUNDATION`. Identità per batch: emissione effettiva = `staker_share + foundation_share` (con `total_staked == 0`: solo `foundation_share`). **Semantica del cap (senza ambiguità, v0.9.2)**: il 15% è il cap ESATTO sulla **quota nominale** di ogni emissione — senza dust, la foundation riceve esattamente `EMISSION − staker_share`, mai un'unità in più. Nei batch con `total_staked == 0` la quota staker (85%) **non nasce**: la foundation riceve comunque solo il suo 15% nominale — che però è il **100% dell'emissione effettiva** di quel batch. La frase "capped al 15%" è vera rispetto all'emissione nominale, non rispetto all'emissione effettiva dei batch senza staker: è esattamente il bootstrap del fair launch, dichiarato.
- **Momento dell'emissione (congelato)**: applicata esclusivamente alla **chiusura** del batch `h`, dopo l'esecuzione di tutte le tx; **non spendibile dalle tx del medesimo batch**.
- **`native_emitted` (congelato)**: conta esclusivamente l'emissione già entrata nello stato economico, nel batch in cui avviene — incluse le quote accreditate alla riserva staking (`staking_reserved`) non ancora reclamate. Non è "reward già pagate".
- Lo shift è intero su `u128`: `EMISSION = 0` da quando `emission_index / HALVING_INTERVAL ≥ 128` (o prima, quando lo shift esaurisce i bit di `EMISSION_0`). L'emissione totale effettiva è la somma discreta batch per batch, ricalcolata dal verificatore nel replay.
- **Cap effettivo esatto** (ora perfettamente allineato all'indice 0-based): `GENESIS_SUPPLY + HALVING_INTERVAL × Σᵢ₌₀..₁₂₇ (EMISSION_0 >> i)`. Limite superiore comodo: `GENESIS_SUPPLY + 2 × EMISSION_0 × HALVING_INTERVAL`. Con i parametri proposti (genesis 0): < 21,03 M — ulteriormente ridotto dai batch con `total_staked == 0`, la cui quota staker non nasce mai.

### 7.3 CreateToken (invariato)
- `name`: 16 byte ASCII stampabile, zero-padded; **nessuna unicità** (l'identità è l'id; l'impersonation di nome è parte del gioco).
- `supply ∈ 1..=MAX_SUPPLY`; tutta al creatore all'esecuzione.

### 7.4 Fee: burn totale (ispirato al meccanismo deflattivo di EIP-1559 — senza base/priority fee: la fee è piatta)
- `FEE_TX` piatta per tx eseguita (`Ok` e `Failed`), **bruciata**: `native_burned += fee`. Le `rejected` non pagano. Una multi-hop paga una sola `FEE_TX`.
- Le fee di swap (`fee_bps`) restano flusso separato, interamente agli LP dell'hop (non bruciate).
- Dinamica monetaria: emissione decrescente contro burn proporzionale all'uso — la supply circolante può diventare deflattiva a regime.

### 7.5 Publish — bacheca dati (oracoli portati dagli utenti)
- `Publish { topic, data }` non tocca lo stato: il dato vive **solo nel blocco**. Il ledger fa da bacheca ordinata e timestampata (round drand = timestamp crittografico); lo stato non cresce di un byte.
- Fee del publish: la `tx_fee` canonica (§5.2), interamente bruciata — vale identica per solvency, `Ok` e `Failed`.
- Autenticazione nativa: la firma ed25519 del publisher è l'identità del feed; la sua storia è tutta on-chain.
- **Ordine canonico del feed (v0.9.1)**: per publish sullo stesso `topic` nello stesso blocco, l'ordine è la **posizione di esecuzione** in `txs` (lo stesso ordine usato dalla scansione di auto-settlement §7.6).
- **Nessuna logica on-chain consuma questi dati** (niente VM): i consumatori sono bot e servizi off-chain — coordinamento, feed di prezzo, settlement per convenzione. Uso tipico: update firmati di un provider esterno (es. feed stile Pyth) ripubblicati da chiunque, con verifica della firma del provider a carico del consumatore.

### 7.6 HTLC — swap atomici cross-chain portati dagli utenti (congelato)

```rust
struct Htlc {
    id: [u8;32],            // blake3(0x04 || sender || LE64(payload.nonce))
    sender: AccountId,
    recipient: AccountId,   // fissato al lock, immutabile
    token: [u8;32],         // NATIVE | TokenId | LpTokenId
    amount: Amount,
    hashlock: [u8;32],      // sha256(preimage) — NON blake3, vedi sotto
    expiry_round: u64,
}
```

**Semantica (congelata)**. `HtlcLock`: debita `amount` dal sender; i fondi vivono nella tabella `htlcs` dello stato — **nessun account li possiede, nemmeno la node key può toccarli**. `HtlcClaim`: valida sse `sha256(preimage) == hashlock` **e** `R <= expiry_round`; accredita `amount` al `recipient` (chiunque può inviarla — conta il preimage, non il mittente). `HtlcRefund`: valida sse `R > expiry_round`; accredita `amount` al `sender`, **invocabile da chiunque** (garbage collection dello stato senza dipendere dal sender). Confine claim/refund netto: `<=` contro `>`, nessuna sovrapposizione. L'HTLC risolto è rimosso dalla tabella.

**Auto-settlement via Publish (congelato, v0.8.1; sequenza pinnata v0.8.3)**. **Dopo l'esecuzione sequenziale di tutte le tx** e **prima dell'emissione**, il nodo scandisce in ordine di esecuzione i `Publish` eseguiti con `Ok` nel batch: (conseguenza: se nello stesso batch un `HtlcClaim` regola l'HTLC durante l'esecuzione, il `Publish` col medesimo preimage trova l'indice vuoto ed è un no-op — il claim vince, deterministicamente) per ogni `data` di **esattamente 32 byte**, se `sha256(data)` corrisponde all'hashlock di un HTLC aperto con `expiry_round >= R`, quell'HTLC si regola verso il `recipient` come un claim (rimozione + accredito). Lookup O(1) su un **indice `hashlock → htlc_id`**: **cache derivabile** dalla tabella `htlcs`, **esclusa dallo state root** (§5.4) e ricostruita deterministicamente al replay — non può divergere senza che diverga la tabella impegnata (aggiornato a lock/claim/refund/settle; hashlock duplicato: il lock successivo con hashlock già indicizzato è `Failed: HtlcDuplicateHashlock` — un hashlock, un HTLC). Proprietà: il preimage è **carrier-independent** — può consegnarlo il recipient, un watchtower, o qualsiasi terzo, anche in più copie ridondanti; il recipient può essere offline; ogni settlement è replay-verificabile (il preimage sta nel blocco). `HtlcClaim` resta come via diretta equivalente.

**Meccanismo, non policy**: il protocollo non conosce l'altra chain né l'accordo tra le parti — fornisce solo il lucchetto condizionale deterministico. Swap, bridge, escrow e watchtower sono protocolli **degli utenti**, costruiti sopra.

**Perché SHA-256**: è l'unico punto del protocollo che non usa blake3 — deliberatamente. Lo swap atomico richiede lo **stesso hash sui due lati**, e SHA-256 è lo standard di Bitcoin script, Lightning, EVM e Solana. Preimage fisso a **32 byte esatti** (standard Lightning): chiude i preimage-length attack noti su Bitcoin script. Dipendenza: crate `sha2` (RustCrypto, famiglia coperta dall'audit NCC 2020).

**Sicurezza — da letteratura, dichiarata**:
- **Censura del claim (classe MAD-HTLC)**: su chain PoW/PoS l'attacco è corrompere i miner perché ignorino il claim fino al timeout; da noi il "miner" è l'operatore unico. Con l'auto-settlement il bersaglio non è più "la tx di Bob": il preimage può arrivare da chiunque, in qualsiasi blob cifrato — per censurarlo il nodo deve scartare **alla cieca e in massa** blob che vede solo a decifratura avvenuta, lasciando ricevute §9.2 che il replay pubblico smaschera. **Caso limite dichiarato**: l'inclusione resta l'unico cancello (§1) — un operatore disposto ad auto-incriminarsi può scartare tutto fino all'expiry; finestre lunghe trasformano questo in un sabotaggio pubblico prolungato, non in un colpo di mano. Il gradino oltre non è un meccanismo più furbo: è un secondo produttore di blocchi, cioè un'altra architettura.
- **Stagger dei timeout (normativo per gli utenti)**: nello swap a due HTLC, il lato che viene reclamato per primo deve scadere **molto prima** del refund dell'altro lato (T2 < T1), con margine per i ritardi di entrambe le chain. Timeout troppo corti sono l'errore classico.
- **Free option / sore loser**: l'HTLC dà a chi conosce il preimage un'opzione gratuita fino all'expiry. È strutturale, non un bug del nostro protocollo; mitigazione pratica: spezzare swap grossi in tranche piccole.
- **Vita massima**: `expiry_round <= R + HTLC_MAX_LIFETIME_ROUNDS` — lo stato non accumula lock eterni.
- **Griefing da hashlock duplicato (threat model)**: "un hashlock, un HTLC" implica che chi locka per primo un hashlock noto blocca gli altri (al costo della propria fee e del proprio capitale lockato). Difesa utente standard: hashlock fresco per ogni swap, mai pre-annunciato in chiaro — il tlock copre comunque il lock fino all'inclusione.

Il protocollo resta senza bridge: gli HTLC sono la primitiva con cui **gli utenti** costruiscono i propri swap contro qualsiasi chain con hashlock, senza custode e senza che POPCORN tocchi mai asset esterni.

---

## 8. Staking — distribuzione O(1)

```
PRECISION = 10^18

per batch (chiusura) — Synthetix StakingRewards letterale (v0.9.2):
    acc_per_stake    += (staker_share * PRECISION) / total_staked   // floor
    staking_reserved += staker_share                                // INTERO: la riserva detiene
                                                                    // tutto il monte reward, come
                                                                    // il contratto Synthetix. Nessuna dust.
    // se total_staked == 0 → staker_share non emessa (§7.2); nessun accredito

pending(a) = (a.staked * (acc_per_stake - a.paid_acc)) / PRECISION  // UNA sola floor,
             // sulla differenza (earned di Synthetix): sempre ≤ entitlement vero

Stake/Unstake/ClaimRewards (ordine congelato):
    1. p = pending(a); PAGA p come TRASFERIMENTO: staking_reserved -= p; balance += p
       // il totale non cambia: B + R = (B+p) + (R−p) — parte del property test
    2. aggiorna a.staked (dopo il regolamento, mai prima)
    3. a.paid_acc = acc_per_stake                                   // snapshot, non un importo

DIMOSTRAZIONE DI SOLVIBILITÀ (v0.9.2): ogni batch aggiunge staker_share sia a
staking_reserved sia al monte-entitlement vero Σ s_a·Δacc/P; ogni settle paga
⌊s·(acc−paid)/P⌋ ≤ entitlement maturato dall'account nell'intervallo. Quindi
Σ pagato + Σ pending ≤ Σ staker_share = staking_reserved cumulato
⇒ staking_reserved ≥ Σ pending ≥ 0, SEMPRE, per costruzione.
```

**Guard di liquidità (congelato)**: `Stake` richiede a runtime, dopo il prelievo della fee, `saldo_nativo ≥ amount + FEE_TX` — deve restare almeno una `FEE_TX` liquida. Senza questo guard un account che staka il 100% resterebbe **permanentemente bloccato**: nessuna tx (nemmeno `Unstake`) supererebbe la fee-solvency, con valore chiuso dentro per sempre.

**Aritmetica (congelato v0.8.3)**: tutte le moltiplicazioni `staked × acc_per_stake` e `staker_share × PRECISION` avvengono in **intermedie U256** (come §6), risultato riconvertito in u128. Bound, tutti incondizionati grazie alla supply finita: i risultati (`pending`; `paid_acc` è un valore di `acc_per_stake`, quindi ne eredita il tappo) rientrano in u128 (≤ ~1,9×10³² nel caso patologico); **`acc_per_stake` stesso è tappato dall'halving** — anche con `total_staked = 1` in ogni batch per l'eternità, il suo massimo è l'intera emissione staker storica × PRECISION ≈ 0,85 × 21,02M × 10⁹ × 10¹⁸ ≈ 1,8×10³⁴, quattro ordini sotto u128::MAX. Nessun campo può sforare, senza ipotesi sull'orizzonte di vita della chain. Le intermedie U256 restano obbligatorie: è il *prodotto* `staked × acc` (fino a ~10⁵⁰) a non stare in u128, non i suoi risultati.

**Nota Unstake a saldo zero**: un account che fa `Unstake` totale riceve stake+pending come saldo liquido, quindi non si blocca; chi comunque arrivasse a saldo < FEE_TX resta inattivo finché non riceve fondi — via d'uscita esterna sempre esistente, dichiarata accettabile.

**Un solo residuo (v0.9.2)**: con la riserva intera, la dust d'accumulatore non esiste più. Resta il **rounding residue** `staking_reserved − Σ pending ≥ 0`: le frazioni che la floor lato-utente lascia nella riserva. È passività del protocollo — non attribuibile senza O(N), quindi né bruciata né regalata: resta lì, dichiarata, e col fix ha finalmente il segno giusto.

**Gate obbligatorio pre-genesis (ESTESO v0.9.2)**: property test consensus-grade — milioni di sequenze casuali di emission/stake/unstake/claim con distribuzioni arbitrarie e PRECISION anche estreme, con verifica **dopo ogni singola operazione** di: (a) invariante a quattro bucket (uguaglianza esatta); (b) conservazione del totale in ogni settle; (c) **`staking_reserved ≥ Σ pending ≥ 0`** — l'asserzione (c) è quella che il gate v0.9.1 non aveva e che avrebbe intercettato il bug.

**Guard di liquidità — wording chiarito (v0.9.1)**: la condizione si valuta DOPO la fase 5b (fee già bruciate): `saldo ≥ amount + FEE_TX`, cioè *dopo lo stake resta almeno una FEE_TX liquida*. Equivalente pre-fee: `saldo ≥ tx_fee + amount + FEE_TX`.

Solo `NATIVE_TOKEN` è stakeabile. Nessun unbonding. Lo stake non è spendibile né trasferibile finché in stake. Lo staking è l'unico modo di partecipare all'emissione: è il "mining" della chain.

---

## 9. API del nodo

### 9.1 Endpoint
| Metodo | Path | Funzione |
|---|---|---|
| `POST` | `/tx` | Submit `{blob: base64, target_round: u64}` → ricevuta firmata |
| `GET` | `/head` | Header ultimo blocco |
| `GET` | `/block/{height}` | Blocco completo (Borsh base64 + JSON) |
| `GET` | `/account/{id}` | Stato account |
| `GET` | `/pair/{id}` | Riserve, fee_bps, lp_supply |
| `GET` | `/tokens`, `/pairs` | Elenchi (pair raggruppate per coppia, tutti i tier) |
| `GET` | `/supply` | GENESIS, emitted, burned, circolante, staked |
| `GET` | `/topic/{topic}?from={h}` | Publish di un topic (indice di convenienza sui blocchi, non stato) |
| `GET` | `/blob/{hash}` | Blob cifrato manifestato (audit della contabilità §5.1; anche su mirror) |
| `GET` | `/chain/export?from={h}` | Stream blocchi per replay |
| `GET` | `/params` | Parametri + chain-info drand + pubkey nodo + pubkey foundation |
| `WS` | `/stream` | Push blocchi |

### 9.2 Ricevuta di sottoscrizione firmata + collection commitment
Risposta a `POST /tx`: la **ricevuta canonica definita sotto** (v0.9.1: `Ed25519(node_key, blake3(borsh(ReceiptPayload)))` — UNICO preimage normativo; qualsiasi concatenazione informale è un'implementazione errata). Con il commitment v0.8.2 la catena di responsabilità diventa a due firme:
- **ricevuta emessa, hash assente dal `blob_manifest`** → due firme dello stesso nodo che si contraddicono: **censura provata dal blocco stesso**, senza bisogno di replay;
- **hash nel manifest** → deve risolversi in `txs`/`rejected`/`unusable` (contabilità §5.1); un claim `unusable` falso è smentibile da chiunque con blob + beacon; il rifiuto di servire un blob manifestato (`GET /blob/{hash}`) è ostruzione visibile.

**Ricevuta canonica (v0.9.1)**: la ricevuta firma `receipt_hash = blake3(borsh(ReceiptPayload{ domain: "popcorn-receipt-v1", blob_hash, target_round, timestamp_ms }))` — wire-canonical, riproducibile in qualsiasi linguaggio. Il `timestamp_ms` è dichiarato dal nodo, **non-consensus**: mai usato per ordinamento, validità o come prova crittografica di orario (§13).

**Protocollo di partecipazione (policy CLIENT, non consenso — v0.9.1)**: se la ricevuta per un blob verso `R` arriva **prima della deadline** che il client si è fissato (≤ `T(R) − ε`), il blob è da considerarsi protetto per `R`; altrimenti il client NON deve fare affidamento sull'inclusione in `R` e può ricifrare verso un round successivo. Il consenso conosce solo blob/round/ricevuta/manifest: la reazione al mancato rilascio della ricevuta è interamente del client.

**Il limite, senza ambiguità**: ricevuta prima della deadline → prova forte di omissione se il blob non appare; nessuna ricevuta → il client può *dire* di aver inviato, ma non ha prova crittografica. POPCORN fornisce accountability forte per i blob ricevutati, non una prova universale della ricezione di ogni pacchetto.

Residuo dichiarato: il **rifiuto cieco all'ingresso** (il nodo non emette la ricevuta) — cieco per costruzione del tlock: il nodo non sa cosa sta rifiutando. Nota DoS (fuori consenso): blob spazzatura non pagano fee; il tetto di ammissione è `MAX_TX_PER_BATCH` più rate-limiting wire-level per IP — superficie accettata e dichiarata.

---

## 10. Verifica di terze parti

Verificatore indipendente (stesso binario, `--verify`):
1. Scarica `/chain/export` (o il mirror).
2. Per blocco: `node_signature` su `block_hash`, `prev_hash`, firma drand del round contro chain-info pubblica quicknet, coerenza `drand_sig_hash`, **ricalcolo di shuffle + normalizzazione (§3, §5.3)**, ricalcolo di `txs_root`/`rejected_root`/`results_root`, **coerenza del collection commitment**: `collection_root` = blake3 del `blob_manifest` ordinato, `unusable ⊆ manifest`, e (con i blob dal mirror) contabilità completa manifest → txs/rejected/unusable, ridecifrando col beacon.
3. Replay da genesis: confronto di ogni `state_root` (§5.4), **verifica della formula di emissione** e dell'**invariante monetario** a ogni blocco.

Qualsiasi divergenza = prova crittografica di scorrettezza. **La specifica definisce le semantiche normative; le implementazioni delle primitive esterne (Ed25519, Borsh, age/tlock, U256) sono accettate solo nella versione/profilo congelato in §13 e devono superare i test vector consensus-grade** — è questo, non la purezza degli algoritmi, a rendere la verifica riproducibile in qualsiasi linguaggio.

**Test vector consensus-grade (obbligo v0.9)**: il repo mantiene fixture byte-per-byte end-to-end — tx firmata → blob age/tlock → beacon reale → batch ordinato → stato risultante → `state_root` → ricevuta — riproducibili da un implementatore indipendente. Accanto al verificatore, un **reference executor minimale e indipendente** (AMM, fee, reward, shuffle, serializzazione dello state root) fa da doppio differenziale: due implementazioni della stessa spec che divergono = bug di spec o di codice, trovato prima del genesis.

**Due proprietà distinte (v0.9.1)**: il **replay dello stato** è self-contained (SignedTx + root + genesis → state_root, basta `/chain/export`); l'**audit della collection** (manifest → tx/rejected/unusable) NON lo è: richiede i blob (dal nodo o dal mirror) e il beacon. Sono garanzie diverse e vanno citate separatamente.

**Perimetro del replay (congelato)**: il replay parte dai `SignedTx` **in chiaro** contenuti nei blocchi — i blob cifrati non vivono nel blocco. Le firme utente rendono le tx non falsificabili dal nodo (può omettere, non inventare). La proprietà "il nodo non vedeva le tx prima del round" è garantita dalla cifratura lato client e dal pinning tlock, e **non è ri-verificabile nel replay**: è l'unica proprietà del sistema che poggia sul comportamento dei client, non sui blocchi. Il mirror dei blob cifrati originali è **obbligo operativo dell'operatore** (fuori consenso, v0.9.3): senza di esso l'audit della collection (§9.2, derivazione di `unusable`) non è praticabile da terzi — un operatore che non pubblica i blob manifestati sta ostruendo l'audit, visibilmente.

---

## 11. Parametri di protocollo (genesis)

| Parametro | Valore proposto | Note |
|---|---|---|
| `BATCH_PERIOD` | 1 round quicknet (3 s) | allineato al beacon |
| `MAX_TX_PER_ACCOUNT_PER_BATCH` | 8 | budget anti-spam |
| `MAX_TX_PER_BATCH` | 10 000 | tetto risorse |
| `MAX_PATH_LEN` | 4 | hop massimi |
| `FEE_TIERS` | {5, 30, 100} bps | unici ammessi |
| `MAX_SUPPLY` | 10³⁰ | tetto supply per token utente |
| `GENESIS_SUPPLY` | 0 | **fair launch**: nessuna allocazione al genesis |
| `EMISSION_0` | 1 × 10⁹ | 1 nativo/batch iniziale (≈ 28 800/giorno) |
| `HALVING_INTERVAL` | 10 512 000 batch | ≈ 1 anno a 3 s/batch |
| `EMISSION_STAKER_BPS` | 8 500 | 85% staker / 15% foundation |
| → supply massima | ≈ 21,02 M | limite superiore; l'effettiva è minore (batch senza staker: quota staker mai nata) |
| `FEE_TX` | 5 000 | 0,000005 nativi (come i 5 000 lamport di Solana), piatta, bruciata |
| `MAX_PUBLISH_SIZE` | 512 B | payload massimo di `Publish` |
| `PUBLISH_FREE_BYTES` | 128 B | soglia inclusa nella fee piatta |
| `PUBLISH_BYTE_FEE` | 50 | unità per byte oltre soglia (512 B ≈ 0,0000242 nativi) |
| `SIGN_DOMAIN` | "popcorn-v1" | dominio di firma, 10 byte ASCII |
| `MINIMUM_LIQUIDITY` | 1 000 | bruciata al primo mint |
| `PRECISION` | 10¹⁸ | accumulatore staking |
| `MAX_BLOB_SIZE` | 2 KiB | payload cifrato |
| `HTLC_MAX_LIFETIME_ROUNDS` | 864 000 | ≈ 30 giorni: vita massima di un lock (§7.6) |
| `BLOB_ROUND_HORIZON` | 200 | ≈ 10 min: il nodo accetta blob solo per round vicini (§3 v0.9) |
| `CONSENSUS_VERSION` | 0x0000_0009_0002 | versione normativa del consenso (§13) |
| `GENESIS_DRAND_ROUND` | fissato al genesis | mapping normativo `round(h) = G + h − 1` (§3 v0.9.1) |
| `DRAND_REMOTES` | api.drand.sh, drand.cloudflare.com | stesso chain-hash, fallback in ordine |

**Requisiti operativi (fuori consenso, congelati come obbligo v0.8.4)** — la fase di raccolta è cieca (nessuna fee prima della decifratura: il signer non è noto), quindi l'availability del nodo va difesa a livello wire, senza toccare il protocollo: `MAX_TOTAL_INGRESS_PER_ROUND` (tetto byte accettati per round, ≥ MAX_TX_PER_BATCH × MAX_BLOB_SIZE), `MAX_BLOBS_PER_CONNECTION`, `MAX_BYTES_PER_IP_WINDOW`, `MAX_TLOCK_DECRYPT_WORK_PER_ROUND` (budget CPU con degradazione dichiarata: la chain ritarda, mai salta blob manifestati). **Benchmark worst-case obbligatorio pre-genesis**: 10k ciphertext validi, 10k ciphertext tlock invalidi, 10k blob garbage — il caso peggiore per CPU può non essere quello ovvio. Questo è un rischio di *availability del nodo*, non di correttezza del ledger: dichiarato e separato.

I valori monetari (GENESIS, EMISSION_0, HALVING, split) sono proposte calibrabili prima del genesis; dopo il genesis sono **immutabili**.

---

## 12. Fuori scope dichiarato

- Nessuna VM / codice utente (porta aperta: `Action` estendibile).
- Nessuna concentrated liquidity, hooks, limit order nativi, flash loan, TWAP on-chain.
- Nessun consenso, P2P, resistenza alla censura hard.
- Nessun oracolo oltre drand. **Nessun bridge di protocollo**: POPCORN non custodisce né verifica asset esterni. Gli HTLC (§7.6) sono la primitiva con cui gli utenti costruiscono swap atomici cross-chain per conto proprio; qualsiasi bridge custodial è attività di terzi, off-chain e fuori protocollo, con le relative responsabilità in capo a chi lo organizza.


---

## 13. POPCORN-CONSENSUS — definizione normativa del consenso (v0.9)

Il determinismo di POPCORN è **normativo, non emergente**: la chain non è deterministica "perché Rust+Borsh+blake3 lo sono", ma perché questo documento definisce esplicitamente ogni semantica che può influenzare lo state root. `Cargo.lock` non è una specifica di consenso: lo è questa tabella.

`CONSENSUS_VERSION = 0x0000_0009_0002` (impressa nel genesis e in `/params`; forma a tre campi 16-bit `0x{riservato}_{minor}_{patch}`: qui minor=9, patch=2 — il patch level segue le revisioni della spec, il freeze fissa il valore definitivo).

| Componente | Definizione normativa |
|---|---|
| Hash | BLAKE3-256 (e XOF per lo shuffle); SHA-256 SOLO per gli hashlock HTLC (§7.6) |
| Serializzazione | Borsh, versione ESATTA in CONSENSUS-LOCK (crate + derive + feature; decodifica delle collection con ordine strettamente crescente obbligatorio — feature `de_strict_order` o check equivalente); solo strutture canoniche (§2) |
| Firma | Ed25519, `ed25519-dalek` versione pinnata, semantica `verify_strict` (§3) |
| Interi | u128 con intermedie U256 (`primitive-types` pinnata); overflow ⇒ `Failed` deterministico, mai panic |
| Ordinamento | sort per tx_id → Fisher-Yates su BLAKE3-XOF(sig‖LE64(height)) con rejection sampling definito (§3) → normalizzazione nonce (§5.3) |
| AMM | POPCORN-V2-MATH (§6): formule, arrotondamenti e lifecycle come scritti, floor ovunque, rounding a favore del pool |
| Fee | `tx_fee` canonica (§5.2), fase unica 5b, burn totale |
| Emissione/reward | §7.2 (indice 0-based, regola no-staker) + §8 (accumulatore, riserva) |
| Timelock | schema tlock su drand quicknet, `DRAND_SCHEME` e `DRAND_CHAIN_HASH` pinnati, formato blob age, policy beacon (§3) — dietro `TimelockProvider`, sostituibile |
| State commitment | hash sequenziale BLAKE3 per tabelle taggate (§5.4) |

**CONSENSUS-LOCK (annex del genesis, v0.9.1)**: al freeze si registra la versione ESATTA (crate, derive, feature flags, commit per i vendorizzati) di: `borsh`+`borsh-derive`, `ed25519-dalek`, `primitive-types`, `blake3`, `sha2`, `age`, `tlock`, `tlock_age`, `drand_core`. "Versione pinnata" senza numero è contrario a questa stessa sezione: i numeri vivono nell'annex, impresso nel genesis accanto a `CONSENSUS_VERSION`.

**Discriminanti normativi (v0.9.1)** — i valori Borsh degli enum di consenso sono tabellati; aggiungere/rimuovere/riordinare varianti è consensus-breaking (i `results_root`/`rejected_root` dipendono dai discriminanti):
`RejectReason`: 0 Malformed, 1 BadSignature, 2 WrongRound, 3 UnknownAccount, 4 PubkeyMismatch, 5 FieldOutOfRange, 6 DuplicateNonce, 7 NonceGap, 8 OverBudget, 9 FeeInsolvent, 10 NonceExhausted.
`FailReason`: 0 InsufficientBalance, 1 SlippageExceeded, 2 UnknownToken, 3 UnknownPair, 4 PairAlreadyExists, 5 LpTokenAsPairSide, 6 ZeroOutput, 7 LiquidityTooSmall, 8 ReGenesisGuard, 9 BadPath, 10 StakeLiquidityGuard, 11 Overflow, 12 SupplyOutOfRange (riservato: unreachable — la statica intercetta prima con FieldOutOfRange), 13 SelfTransferNoop, 14 HtlcNotFound, 15 HtlcBadPreimage, 16 HtlcExpired, 17 HtlcNotExpired, 18 HtlcDuplicateHashlock.
`ExecStatus`: 0 Ok, 1 Failed(FailReason). `Action`: i discriminanti seguono l'ordine di §4.3, tabellati nell'annex.

**Overflow per-caso (v0.9.1)** — "overflow ⇒ Failed" è troppo generico; semantica congelata: intermedie U256 in AMM/staking che sforano al rientro in u128 ⇒ `Failed(Overflow)`; `supply` fuori range ⇒ statica (`FieldOutOfRange`); `account.nonce == u64::MAX` ⇒ account **terminale**: ogni sua tx ⇒ `rejected: NonceExhausted` (mai un incremento che wrappa); `native_emitted`/`native_burned`/`acc_per_stake`/`staking_reserved` sono bounded dalla supply finita (§8) e un loro overflow è irraggiungibile per costruzione — un implementatore li tratta comunque con aritmetica checked e un overflow lì è un bug fatale (halt), mai un wrap silenzioso.

**Cambiamenti NON consensus-breaking** (liberi): implementazione HTTP/WS, logging, metriche, compaction del database, RPC, CLI, mirror, rate-limiting wire-level.

**Cambiamenti consensus-breaking** (richiedono nuovo `CONSENSUS_VERSION` e, post-genesis, sono di fatto una nuova chain): semantica di verifica delle firme, serializzazione, algoritmi di hash, ordine di attraversamento dello stato, arrotondamenti AMM, calcolo delle fee, formule di emissione/reward, algoritmo di shuffle, mapping round→blocco, policy del beacon. **Nessun aggiornamento di dipendenza crittografica entra nel consenso automaticamente**: l'upgrade di una versione pinnata in questa tabella è un cambio di consenso dichiarato, mai un side-effect di `cargo update`.

**Nota drand (rischio dichiarato)**: la rete drand è operativa (release 2.1.x nel 2026) ma è infrastruttura esterna la cui continuità non è sotto il controllo di POPCORN — lo steward Randamu è stato chiuso a febbraio 2026. Mitigazioni: orizzonte timelock breve (§3), `TimelockProvider` sostituibile, e il precedente fastnet come promemoria che un sunset impatta l'availability, non i fondi (nessun ciphertext a lungo termine esiste per costruzione).

**Roadmap dichiarata post-freeze (P3, non sicurezza)**: migrazione `primitive-types` → `alloy-primitives`/`ruint` SOLO dopo il freeze, con test differenziali byte-per-byte contro l'implementazione corrente — cambiare l'aritmetica di un consensus engine funzionante è un rischio, non un fix.
