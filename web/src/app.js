// The POPCORN explorer and wallet.
//
// Two halves that share one client: a read-only explorer over the endpoints of §9.1, and a
// wallet that runs the client flow of §3.2 — sign, timelock-encrypt, submit blind, keep the
// receipt. The page is served by the node itself, so it is same-origin and there is nothing
// to configure.

import { NodeClient } from "./api.js";
import { ACTION_FORMS, FORM_BY_KIND, coerce } from "./actions.js";
import {
  LocalSigner, NATIVE_TOKEN, WalletSigner, accountId, fromHex, toBase58, toHex,
} from "./popcorn.js";
import { awaitInclusion, sendAction } from "./wallet.js";
import { banner, clear, groupDigits, h, panel, rows, shortHash, stat, table } from "./ui.js";

const client = new NodeClient(globalThis.POPCORN_API ?? "");
const NATIVE_HEX = toHex(NATIVE_TOKEN);

const state = {
  params: null,
  head: null,
  liveBlocks: [],
  connection: "connecting",
  signer: null,
  account: null,
  tokenNames: new Map(),
  status: null,
};

// ------------------------------------------------------------------------------ helpers

/** A token's display name, falling back to its id — the native token has neither. */
function tokenLabel(id) {
  if (id === NATIVE_HEX) return "POP";
  const name = state.tokenNames.get(id);
  return name ? `${name}` : `${id.slice(0, 8)}…`;
}

function route() {
  const hash = globalThis.location.hash.replace(/^#\/?/, "");
  const [view, ...rest] = hash.split("/");
  return { view: view || "overview", arg: rest.join("/") };
}

function go(path) {
  globalThis.location.hash = `#/${path}`;
}

function blockLink(height) {
  return h("a", { href: `#/block/${height}`, class: "mono" }, String(height));
}

async function refreshTokenNames() {
  try {
    const { tokens } = await client.tokens();
    state.tokenNames = new Map(tokens.map((token) => [token.id, token.name]));
  } catch {
    // Names are decoration; ids always work.
  }
}

// ------------------------------------------------------------------------------- chrome

function header() {
  const status = {
    live: ["ok", "live"],
    connecting: ["wait", "connecting"],
    reconnecting: ["wait", "reconnecting"],
    offline: ["bad", "offline"],
  }[state.connection] ?? ["mute", state.connection];

  return h("header", { class: "top" },
    h("img", { src: "logo.jpg", alt: "POPCORN" }),
    h("div", {},
      h("div", { class: "title", text: "POPCORN" }),
      h("div", { class: "sub", text: "deterministic execution, verifiable by anyone" })),
    h("div", { class: "spacer" }),
    h("div", { class: "right" },
      h("span", { class: `pill ${status[0]}`, text: status[1] }),
      h("div", { class: "sub mono mt-xs" },
        state.head ? `height ${state.head.height} · round ${state.head.drand_round}` : "…")));
}

function nav(current) {
  const items = [
    ["overview", "Overview"],
    ["blocks", "Blocks"],
    ["tokens", "Tokens"],
    ["pairs", "Pairs"],
    ["topics", "Data board"],
    ["supply", "Supply"],
    ["wallet", "Wallet"],
    ["verify", "Verify"],
  ];
  return h("nav", { class: "tabs" },
    items.map(([key, label]) =>
      h("a", { href: `#/${key}`, class: key === current ? "on" : null, text: label })));
}

function searchBar() {
  const input = h("input", {
    placeholder: "block height, account id, pair id, token id, topic or blob hash",
    spellcheck: "false",
  });
  const submit = async () => {
    const query = input.value.trim();
    if (query.length === 0) return;
    if (/^[0-9]+$/.test(query)) return go(`block/${query}`);
    if (!/^[0-9a-fA-F]{64}$/.test(query)) {
      state.status = banner("error", "a search is a block height or 32 bytes of hex");
      return render();
    }
    const id = query.toLowerCase();
    // Ids live in different namespaces by construction (§4.1), so at most one of these can
    // answer — asking in turn is not ambiguous, just a few requests.
    for (const [probe, destination] of [
      [() => client.account(id), `account/${id}`],
      [() => client.pair(id), `pair/${id}`],
    ]) {
      try {
        await probe();
        return go(destination);
      } catch { /* try the next namespace */ }
    }
    go(`topic/${id}`);
  };
  input.addEventListener("keydown", (event) => { if (event.key === "Enter") submit(); });
  return h("div", { class: "searchbar" }, input, h("button", { onclick: submit }, "Look up"));
}

// ----------------------------------------------------------------------------- explorer

async function viewOverview() {
  const [supply, pairs] = await Promise.all([
    client.supply().catch(() => null),
    client.pairs().catch(() => ({ pairs: [] })),
  ]);
  const head = state.head ?? {};

  const invariant = supply
    ? h("span", { class: `pill ${supply.invariant_holds ? "ok" : "bad"}` },
        supply.invariant_holds ? "invariant holds" : "INVARIANT BROKEN")
    : h("span", { class: "pill mute" }, "unknown");

  return [
    searchBar(),
    h("div", { class: "grid stats" },
      stat("Height", String(head.height ?? "…")),
      stat("Drand round", String(head.drand_round ?? "…")),
      stat("Accounts", String(supply?.accounts ?? "…")),
      stat("Pairs", String(pairs.pairs?.length ?? 0))),
    h("div", { class: "grid two mt" },
      panel("Chain head",
        "Every root here is recomputable from the exported blocks alone.",
        rows([
          ["block hash", shortHash(head.block_hash)],
          ["state root", shortHash(head.state_root)],
          ["collection root", shortHash(head.collection_root)],
          ["txs root", shortHash(head.txs_root)],
          ["results root", shortHash(head.results_root)],
          ["previous", head.height ? blockLink(Number(head.height) - 1) : "—"],
        ])),
      panel("Native supply",
        "Five buckets, exact equality by construction (§5.5).",
        rows([
          ["invariant", invariant],
          ["emitted", groupDigits(supply?.emitted)],
          ["burned", groupDigits(supply?.burned)],
          ["circulating", groupDigits(supply?.circulating)],
          ["staked", groupDigits(supply?.staked)],
          ["in HTLCs", groupDigits(supply?.in_htlcs)],
          ["in pools", groupDigits(supply?.in_pools)],
          ["staking reserve", groupDigits(supply?.staking_reserved)],
        ]))),
    h("div", { class: "mt" },
      panel("Live blocks",
        "Pushed over the node's websocket as they are produced. One block per drand round.",
        liveBlockTable())),
  ];
}

function liveBlockTable() {
  return table(
    ["height", "round", "txs", { label: "rejected", num: false }, "unusable", "state root"],
    state.liveBlocks.slice(0, 12).map((block) =>
      h("tr", {},
        h("td", {}, blockLink(block.height)),
        h("td", { text: String(block.drand_round) }),
        h("td", { text: String(block.txs) }),
        h("td", { text: String(block.rejected) }),
        h("td", { text: String(block.unusable) }),
        h("td", {}, shortHash(block.state_root)))));
}

async function viewBlocks() {
  const head = state.head ?? (await client.head());
  const top = Number(head.height ?? 0);
  const wanted = [];
  for (let height = top; height > Math.max(-1, top - 25); height -= 1) wanted.push(height);
  const blocks = await Promise.all(wanted.map((height) => client.block(height).catch(() => null)));

  return panel("Recent blocks", "Newest first. Click a height for the full block.",
    table(["height", "round", "txs", "rejected", "manifest", "unusable", "block hash"],
      blocks.filter(Boolean).map((block) =>
        h("tr", {},
          h("td", {}, blockLink(block.header.height)),
          h("td", { text: String(block.header.drand_round) }),
          h("td", { text: String(block.tx_ids.length) }),
          h("td", { text: String(block.rejected.length) }),
          h("td", { text: String(block.blob_manifest.length) }),
          h("td", { text: String(block.unusable.length) }),
          h("td", {}, shortHash(block.block_hash))))));
}

async function viewBlock(height) {
  const block = await client.block(height);
  const header = block.header;
  const executed = block.tx_ids.map((id, index) =>
    h("tr", {},
      h("td", { text: String(index) }),
      h("td", {}, shortHash(id)),
      h("td", { text: block.results[index] ?? "" })));

  return [
    h("div", { class: "actions mb" },
      h("button", { class: "ghost", onclick: () => go(`block/${Number(height) - 1}`) }, "← previous"),
      h("button", { class: "ghost", onclick: () => go(`block/${Number(height) + 1}`) }, "next →")),
    panel(`Block ${header.height}`,
      "Execution order inside a block is the shuffle of §3.7, seeded by the round's beacon.",
      rows([
        ["block hash", shortHash(header.block_hash)],
        ["previous", shortHash(header.prev_hash)],
        ["drand round", String(header.drand_round)],
        ["beacon hash", shortHash(header.drand_sig_hash)],
        ["state root", shortHash(header.state_root)],
        ["collection root", shortHash(header.collection_root)],
        ["node signature", shortHash(block.node_signature)],
      ])),
    h("div", { class: "grid two mt" },
      panel("Executed", `${block.tx_ids.length} transaction(s), in execution order.`,
        table(["#", "tx id", "result"], executed)),
      panel("Rejected", "A rejection is a consensus fact: the reason is committed to (§5.2).",
        table(["tx id", "reason"], block.rejected.map((entry) =>
          h("tr", {},
            h("td", {}, shortHash(entry.tx_id)),
            h("td", { text: entry.reason })))))),
    h("div", { class: "grid two mt" },
      panel("Blob manifest",
        "Everything the node collected for this round, readable or not — this is what makes censorship visible.",
        table(["blob hash"], block.blob_manifest.map((hash) =>
          h("tr", {}, h("td", {}, shortHash(hash)))))),
      panel("Unusable",
        "Collected, but outside POPCORN-TLOCK-AGE-V1 or not a valid transaction for this round.",
        table(["blob hash"], block.unusable.map((hash) =>
          h("tr", {}, h("td", {}, shortHash(hash))))))),
  ];
}

async function viewAccount(id) {
  let account;
  try {
    account = await client.account(id);
  } catch (error) {
    if (error.status === 404) {
      return panel("Account", "",
        banner("info", "This account does not exist yet: it has never received funds. " +
          "Accounts are implicit — the first credit creates one (§4.2)."),
        rows([["account id", shortHash(id)]]));
    }
    throw error;
  }

  return [
    panel("Account", "An account id is blake3 of a public key; the key itself appears only once it signs.",
      rows([
        ["account id", shortHash(account.id)],
        ["public key", account.pubkey ? shortHash(account.pubkey) : h("span", { class: "muted" }, "not yet materialized")],
        ["last nonce", String(account.nonce)],
        ["staked", groupDigits(account.staked)],
        ["pending rewards", groupDigits(account.pending_rewards)],
      ])),
    h("div", { class: "mt" },
      panel("Balances", "",
        table(["token", "id", { label: "amount", num: true }],
          account.balances.map((entry) =>
            h("tr", {},
              h("td", { text: tokenLabel(entry.token) }),
              h("td", {}, shortHash(entry.token)),
              h("td", { class: "num", text: groupDigits(entry.amount) })))))),
  ];
}

async function viewTokens() {
  const { tokens } = await client.tokens();
  return panel("Tokens", "Fixed supply, minted once at creation (§7.3).",
    table(["name", "id", "creator", { label: "total supply", num: true }],
      tokens.map((token) =>
        h("tr", {},
          h("td", { text: token.name || "—" }),
          h("td", {}, shortHash(token.id)),
          h("td", {}, shortHash(token.creator, `#/account/${token.creator}`)),
          h("td", { class: "num", text: groupDigits(token.total_supply) })))));
}

async function viewPairs() {
  const { pairs } = await client.pairs();
  return panel("AMM pairs", "POPCORN-V2-MATH: the constant-product rule generalized to fee tiers (§6).",
    table(["pair", "token0", "token1", "fee", { label: "reserve0", num: true }, { label: "reserve1", num: true }, { label: "LP supply", num: true }],
      pairs.map((pair) =>
        h("tr", {},
          h("td", {}, shortHash(pair.id, `#/pair/${pair.id}`)),
          h("td", { text: tokenLabel(pair.token0) }),
          h("td", { text: tokenLabel(pair.token1) }),
          h("td", { text: `${pair.fee_bps} bps` }),
          h("td", { class: "num", text: groupDigits(pair.reserve0) }),
          h("td", { class: "num", text: groupDigits(pair.reserve1) }),
          h("td", { class: "num", text: groupDigits(pair.lp_supply) })))));
}

async function viewPair(id) {
  const pair = await client.pair(id);
  return panel("Pair", "Reserves are the pool's own bucket in the monetary invariant.",
    rows([
      ["pair id", shortHash(pair.id)],
      ["token0", shortHash(pair.token0)],
      ["token1", shortHash(pair.token1)],
      ["fee", `${pair.fee_bps} bps`],
      ["reserve0", groupDigits(pair.reserve0)],
      ["reserve1", groupDigits(pair.reserve1)],
      ["LP supply", groupDigits(pair.lp_supply)],
      ["LP token", shortHash(pair.lp_token)],
    ]));
}

function viewTopics() {
  const input = h("input", { placeholder: "topic — 32 bytes of hex", spellcheck: "false" });
  const open = () => {
    const value = input.value.trim();
    if (/^[0-9a-fA-F]{64}$/.test(value)) go(`topic/${value.toLowerCase()}`);
  };
  input.addEventListener("keydown", (event) => { if (event.key === "Enter") open(); });
  return panel("Data board",
    "Publishes carry no state effect: the entry lives in the block, in execution order (§7.5). " +
    "A 32-byte publish that matches a live hashlock settles that HTLC — which is how the chain " +
    "learns a secret without watching any other chain.",
    h("div", { class: "searchbar" }, input, h("button", { onclick: open }, "Open topic")));
}

async function viewTopic(id) {
  const { entries } = await client.topic(id);
  const decoder = new TextDecoder();
  return panel(`Topic ${id.slice(0, 12)}…`, `${entries.length} entr${entries.length === 1 ? "y" : "ies"}, oldest first.`,
    table(["block", "round", "#", "publisher", "data"],
      entries.map((entry) => {
        const bytes = Uint8Array.from(atob(entry.data_base64), (character) => character.charCodeAt(0));
        const printable = bytes.every((byte) => byte >= 0x20 && byte < 0x7f);
        return h("tr", {},
          h("td", {}, blockLink(entry.height)),
          h("td", { text: String(entry.drand_round) }),
          h("td", { text: String(entry.position) }),
          h("td", {}, shortHash(entry.publisher, `#/account/${entry.publisher}`)),
          h("td", { text: printable ? decoder.decode(bytes) : toHex(bytes) }));
      })));
}

async function viewSupply() {
  const supply = await client.supply();
  const buckets = [
    ["circulating balances", supply.circulating],
    ["staked", supply.staked],
    ["HTLC escrow", supply.in_htlcs],
    ["AMM pool reserves", supply.in_pools],
    ["staking reserve", supply.staking_reserved],
  ];
  return [
    panel("Monetary invariant",
      "balances + staked + HTLC escrow + pool reserves + staking reserve = genesis + emitted − burned. " +
      "Exact equality, checked after every batch — a missing bucket here is how the reference " +
      "executor caught a real bug before genesis.",
      rows([
        ["holds", h("span", { class: `pill ${supply.invariant_holds ? "ok" : "bad"}` },
          supply.invariant_holds ? "yes" : "NO")],
        ["bucket total", groupDigits(supply.bucket_total)],
        ["genesis + emitted − burned",
          groupDigits((BigInt(supply.genesis_supply) + BigInt(supply.emitted) - BigInt(supply.burned)).toString())],
      ])),
    h("div", { class: "mt" },
      panel("Where the native supply is",
        "Everything outside the first row exists but is not spendable from a balance.",
        table(["bucket", { label: "amount", num: true }],
          buckets.map(([label, value]) =>
            h("tr", {},
              h("td", { text: label }),
              h("td", { class: "num", text: groupDigits(value) })))))),
    h("div", { class: "mt" },
      panel("Emission",
        "The only source of new supply is the per-batch emission with halving (§7.2). Fees are burned, never paid.",
        rows([
          ["emitted so far", groupDigits(supply.emitted)],
          ["burned", groupDigits(supply.burned)],
          ["upper bound", groupDigits(supply.upper_bound)],
          ["height", String(supply.height)],
        ]))),
  ];
}

function viewVerify() {
  const params = state.params ?? {};
  return [
    panel("Verify this chain yourself",
      "Nothing on this page has to be believed. The node exports every block; replaying them " +
      "re-derives every root, and the collection audit checks that the operator served the " +
      "blobs it committed to.",
      h("pre", { class: "mono code-block" },
`# replay every block and recompute every root
popcorn verify --node ${globalThis.location.origin}

# also re-download each manifested blob and re-derive the collection root
popcorn verify --node ${globalThis.location.origin} --audit-collection`)),
    h("div", { class: "mt" },
      panel("Pinned parameters",
        "These are consensus, not configuration: changing one makes a different chain.",
        rows([
          ["consensus version", params.consensus_version ?? "…"],
          ["signing domain", params.sign_domain ?? "…"],
          ["drand scheme", params.drand?.scheme ?? "…"],
          ["drand chain", shortHash(params.drand?.chain_hash)],
          ["genesis round", String(params.genesis_drand_round ?? "…")],
          ["node public key", shortHash(params.node_pubkey)],
          ["foundation account", shortHash(params.foundation_account, `#/account/${params.foundation_account}`)],
          ["max blob size", `${params.parameters?.max_blob_size ?? "…"} bytes`],
          ["tx fee", groupDigits(params.parameters?.fee_tx)],
          ["fee tiers", (params.parameters?.fee_tiers ?? []).join(", ")],
        ]))),
  ];
}

// ------------------------------------------------------------------------------- wallet

async function loadAccount() {
  if (!state.signer) return;
  const id = toHex(accountId(state.signer.pubkey));
  try {
    state.account = { id, ...(await client.account(id)) };
  } catch (error) {
    state.account = { id, missing: error.status === 404, error: error.message };
  }
}

async function connectWallet() {
  state.signer = await WalletSigner.connect();
  await loadAccount();
  render();
}

async function useLocalKey(secretHex) {
  state.signer = new LocalSigner(secretHex ? fromHex(secretHex) : undefined);
  await loadAccount();
  render();
}

function walletConnect() {
  const secret = h("input", { placeholder: "optional: 32-byte secret key in hex", spellcheck: "false" });
  const useButton = h("button", { class: "ghost" }, "Generate a key");
  useButton.addEventListener("click", () => useLocalKey(secret.value.trim() || null).catch((error) => {
    state.status = banner("error", error.message);
    render();
  }));
  secret.addEventListener("input", () => {
    useButton.textContent = secret.value.trim() ? "Use this key" : "Generate a key";
  });
  return [
    panel("Connect",
      "A wallet signs the 32-byte hash blake3(\"popcorn-v1\" || borsh(payload)) and nothing else. " +
      "No Solana RPC is emulated and no Solana transaction is built — the domain prefix is what " +
      "keeps a signature made here from replaying anywhere else (§3.1).",
      h("div", { class: "actions" },
        h("button", {
          onclick: () => connectWallet().catch((error) => {
            state.status = banner("error", error.message);
            render();
          }),
        }, "Connect a Solana wallet"),
        h("span", { class: "muted", text: WalletSigner.available() ? "detected in this browser" : "none detected" }))),
    h("div", { class: "mt" },
      panel("Or use a key in this tab",
        "Held in memory only, and gone when you close the tab. Good for trying the chain; " +
        "not a place to keep anything you care about.",
        h("div", { class: "field" }, secret),
        h("div", { class: "actions" }, useButton))),
  ];
}

function walletForms() {
  const account = state.account ?? {};
  const kindSelect = h("select", {},
    ACTION_FORMS.map((form) => h("option", { value: form.kind, text: form.title })));
  const formHost = h("div", {});
  const output = h("div", {});

  const renderForm = () => {
    const form = FORM_BY_KIND.get(kindSelect.value);
    const inputs = new Map();
    const fields = form.fields.map((spec) => {
      const control = spec.kind === "path" || spec.kind === "data"
        ? h("textarea", { spellcheck: "false", placeholder: spec.hint })
        : h("input", { spellcheck: "false", placeholder: spec.hint });
      if (spec.kind === "token") control.value = NATIVE_HEX;
      if (spec.kind === "fee") control.value = "30";
      inputs.set(spec.name, control);

      let extra = null;
      if (spec.kind === "data") {
        // Whether the field is text or hex cannot be read off the bytes, so it is a choice
        // the user makes — pre-made for them when what they typed can only be a hash.
        const hex = h("input", { type: "checkbox", class: "check" });
        inputs.set(`${spec.name}Hex`, { get value() { return hex.checked; } });
        control.addEventListener("input", () => {
          if (/^[0-9a-fA-F]{64}$/.test(control.value.trim())) hex.checked = true;
        });
        extra = h("label", { class: "check-label" },
          hex, "raw bytes (hex)");
      }

      return h("div", { class: "field" },
        h("label", {}, spec.label, spec.hint ? h("span", { class: "hint", text: ` — ${spec.hint}` }) : null),
        control,
        extra);
    });

    const send = async () => {
      clear(output);
      let action;
      const notes = [];
      try {
        const raw = {};
        for (const [name, control] of inputs) raw[name] = control.value;
        action = coerce(form.kind, raw, notes);
      } catch (error) {
        output.append(banner("error", error.message));
        return;
      }

      const progress = h("div", { class: "banner info" },
        h("span", { class: "spinner" }), " preparing");
      output.append(progress);
      const button = formHost.querySelector("button.send");
      button.disabled = true;

      try {
        const fromHeight = Number(state.head?.height ?? 0);
        const submission = await sendAction(client, state.signer, action, {
          accountId: account.id,
          signerBytes: fromHex(account.id),
          onStage: (stage) => {
            clear(progress);
            progress.append(h("span", { class: "spinner" }), ` ${stage}`);
          },
        });
        clear(output);
        output.append(receiptPanel(submission, notes));

        const waiting = h("div", { class: "banner info" },
          h("span", { class: "spinner" }), " waiting for the target round");
        output.append(waiting);
        const inclusion = await awaitInclusion(client, submission, { fromHeight });
        waiting.replaceWith(inclusionPanel(inclusion));
        await loadAccount();
      } catch (error) {
        clear(output);
        output.append(banner("error", error.message));
      } finally {
        button.disabled = false;
      }
    };

    clear(formHost).append(
      h("p", { class: "note", text: form.blurb }),
      ...fields,
      h("div", { class: "actions" },
        h("button", { class: "send", onclick: send }, "Sign, encrypt and submit")));
  };

  kindSelect.addEventListener("change", renderForm);
  renderForm();

  return panel("Send a transaction",
    "The payload is signed, then encrypted toward a future drand round, then submitted blind: " +
    "the node queues bytes it cannot read and signs a receipt for them (§9.2).",
    h("div", { class: "field" }, h("label", {}, "Action"), kindSelect),
    formHost,
    output);
}

function receiptPanel(submission, notes = []) {
  return panel("Submitted",
    notes.length > 0
      ? "Keep this receipt — and the preimage below, which cannot be recovered from the " +
        "transaction: without it the escrow can only be refunded after expiry (§9.2, §7.6)."
      : "Keep this receipt. If the blob never appears in the manifest of its target round, the " +
        "receipt is signed evidence of that (§9.2).",
    rows([
      ["nonce", String(submission.nonce)],
      ["target round", String(submission.targetRound)],
      ["signing hash", shortHash(submission.signingHash)],
      ["tx id", shortHash(submission.txId)],
      ["blob hash", shortHash(submission.blobHash)],
      ["blob size", `${submission.blobBytes} bytes`],
      ["receipt hash", shortHash(submission.receipt?.receipt_hash)],
      ["node signature", shortHash(submission.receipt?.signature)],
      ...notes.map(([label, value]) => [label, shortHash(value)]),
      ...submission.derived.map(([label, value]) => [label, shortHash(value)]),
    ]));
}

function inclusionPanel(inclusion) {
  const kinds = {
    executed: ["ok", "executed"],
    rejected: ["bad", "rejected"],
    unusable: ["bad", "unusable"],
    manifested: ["wait", "manifested"],
    timeout: ["wait", "not seen yet"],
  };
  const [tone, label] = kinds[inclusion.status] ?? ["mute", inclusion.status];
  const detail = inclusion.reason ?? inclusion.result ?? "";
  return panel("Outcome",
    "Checked against the block itself, not against the node's word for it: the blob hash in the " +
    "manifest, the tx id among the executed ids.",
    rows([
      ["status", h("span", { class: `pill ${tone}`, text: label })],
      inclusion.height ? ["block", blockLink(inclusion.height)] : null,
      detail ? ["detail", String(detail)] : null,
    ]));
}

function walletAccountPanel() {
  const account = state.account ?? {};
  const balances = account.balances ?? [];
  return panel("Your account",
    `Signing with your ${state.signer.label}.`,
    rows([
      ["account id", shortHash(account.id, `#/account/${account.id}`)],
      ["public key", shortHash(toBase58(state.signer.pubkey))],
      ["next nonce", account.missing ? "1" : String(BigInt(account.nonce ?? 0) + 1n)],
      ["staked", groupDigits(account.staked ?? "0")],
      ["pending rewards", groupDigits(account.pending_rewards ?? "0")],
      ...balances.map((entry) => [tokenLabel(entry.token), groupDigits(entry.amount)]),
    ]),
    account.missing
      ? banner("info", "This account has no state yet. It can still sign — the first credit creates it (§4.2).")
      : null,
    h("div", { class: "actions mt-sm" },
      h("button", { class: "ghost", onclick: () => loadAccount().then(render) }, "Refresh"),
      h("button", {
        class: "ghost",
        onclick: () => { state.signer = null; state.account = null; render(); },
      }, "Disconnect")));
}

function viewWallet() {
  if (!state.signer) return walletConnect();
  return [walletAccountPanel(), h("div", { class: "mt" }, walletForms())];
}

// ------------------------------------------------------------------------------- render

const VIEWS = {
  overview: viewOverview,
  blocks: viewBlocks,
  block: viewBlock,
  account: viewAccount,
  tokens: viewTokens,
  pairs: viewPairs,
  pair: viewPair,
  topics: viewTopics,
  topic: viewTopic,
  supply: viewSupply,
  verify: viewVerify,
  wallet: viewWallet,
};

const TAB_FOR_VIEW = { block: "blocks", pair: "pairs", topic: "topics", account: "overview" };

const root = document.querySelector("#app");
let renderToken = 0;

async function render() {
  const { view, arg } = route();
  const token = ++renderToken;
  const shell = h("div", { class: "shell" },
    header(),
    nav(TAB_FOR_VIEW[view] ?? view),
    state.status,
    h("div", { class: "body" }, h("div", { class: "banner info" }, h("span", { class: "spinner" }), " loading")),
    h("footer", { class: "bottom" },
      h("a", { href: "https://github.com/arabafenice599rae/popcorn", target: "_blank", rel: "noreferrer" }, "Source"),
      h("a", { href: "/params" }, "/params"),
      h("a", { href: "/chain/export?from=0" }, "/chain/export"),
      h("span", {}, "served by the node itself — same origin, no third party")));
  state.status = null;
  clear(root).append(shell);

  const body = shell.querySelector(".body");
  try {
    const handler = VIEWS[view] ?? VIEWS.overview;
    const content = await handler(arg);
    if (token !== renderToken) return;
    clear(body).append(...[content].flat().filter(Boolean));
  } catch (error) {
    if (token !== renderToken) return;
    clear(body).append(banner("error", error.message));
  }
}

async function boot() {
  globalThis.addEventListener("hashchange", render);
  render();

  try {
    state.params = await client.params();
    state.head = await client.head();
    state.connection = "connecting";
  } catch {
    state.connection = "offline";
  }
  await refreshTokenNames();
  render();

  client.stream(
    (block) => {
      state.liveBlocks.unshift(block);
      state.liveBlocks = state.liveBlocks.slice(0, 40);
      state.head = { ...state.head, height: block.height, drand_round: block.drand_round,
        block_hash: block.block_hash, state_root: block.state_root,
        collection_root: block.collection_root };
      if (route().view === "overview") render();
      else {
        const badge = root.querySelector("header.top .sub.mono");
        if (badge) badge.textContent = `height ${block.height} · round ${block.drand_round}`;
      }
    },
    (connection) => {
      state.connection = connection;
      const pill = root.querySelector("header.top .pill");
      if (pill) render();
    });

  // The head can also move while the socket is down, so keep a slow poll underneath it.
  setInterval(async () => {
    try {
      state.head = await client.head();
      if (route().view === "overview" && state.connection !== "live") render();
    } catch { /* the node will come back */ }
  }, 15000);
}

boot();
