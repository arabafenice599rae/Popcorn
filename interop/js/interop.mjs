// The JavaScript half of the cross-language gate (SPEC.md §2.4).
//
// Same command surface as the Rust and Go tools, so the harness can run every direction
// between the three implementations.
//
// One thing this file exists to make visible: tlock-js emits **armored** age output
// (`-----BEGIN AGE ENCRYPTED FILE-----`), and POPCORN-TLOCK-AGE-V1 forbids armor. Armor is a
// second encoding of the same ciphertext, so allowing it would give one transaction two blob
// hashes — and the manifest and the receipts both key on that hash. A browser client must
// therefore de-armor before submitting, which is the three lines in `dearmor()` below. It is
// a client requirement, not a consensus change.
//
//   node interop.mjs encrypt <round> <plaintext-hex>
//   node interop.mjs decrypt <blob-hex> <signature-hex>
//   node interop.mjs profile <blob-hex> <round> <chain-hash-hex>

import { timelockEncrypt, timelockDecrypt, defaultChainInfo, Buffer } from "tlock-js";
import { createHash } from "node:crypto";

const CHAIN_HASH = "52db9ba70e0cc0f6eaf7803dd07447a1f5477735fd3f661792ba94600c84e971";
const ARMOR_BEGIN = "-----BEGIN AGE ENCRYPTED FILE-----";
const ARMOR_END = "-----END AGE ENCRYPTED FILE-----";

/// An offline client: chain info is pinned, and the round's signature is supplied by the
/// caller. No HTTP, so the gate never depends on drand being up.
function offlineClient(signatureHex) {
  return {
    chain: () => ({ info: async () => defaultChainInfo }),
    // A well-formed beacon, randomness included: drand defines it as sha256 of the
    // signature, and the client checks that before it checks anything else.
    get: async (round) => ({
      round,
      signature: signatureHex ?? "",
      randomness: signatureHex
        ? createHash("sha256").update(Buffer.from(signatureHex, "hex")).digest("hex")
        : "",
    }),
    // Verification stays ON: the supplied signature is a real quicknet beacon, so this path
    // also checks that the JS side validates the beacon against the pinned chain info,
    // rather than trusting whatever it was handed.
    options: { disableBeaconVerification: false },
  };
}

/// Strip the PEM wrapper and decode: what a browser client must do before `POST /tx`.
function dearmor(armored) {
  const body = armored
    .split("\n")
    .filter((line) => line.length > 0 && !line.startsWith("-----"))
    .join("");
  return Buffer.from(body, "base64");
}

/// The inverse, for handing bytes back to a library that expects armor.
function armor(binary) {
  const body = Buffer.from(binary).toString("base64");
  const lines = body.match(/.{1,64}/g) ?? [];
  return [ARMOR_BEGIN, ...lines, ARMOR_END, ""].join("\n");
}

/// An independent implementation of POPCORN-TLOCK-AGE-V1 (SPEC.md §3.6).
///
/// Written from the specification, not ported from the Rust: the acceptance policy is
/// normative, and two implementations that disagree about which blobs are `unusable`
/// disagree about which transactions exist. Returns "" when the blob conforms.
function validateProfile(blob, expectedRound, expectedChain) {
  if (blob.length === 0) return "Empty";
  if (blob.subarray(0, ARMOR_BEGIN.length).toString("latin1") === ARMOR_BEGIN) return "Armored";

  const lines = [];
  let offset = 0;
  let macSeen = false;
  while (offset < blob.length) {
    const index = blob.indexOf(0x0a, offset);
    if (index < 0) return "MissingMac";
    const line = blob.subarray(offset, index);
    if (line.includes(0x0d)) return "CarriageReturn";
    lines.push(line.toString("latin1"));
    offset = index + 1;
    if (lines[lines.length - 1].startsWith("---")) {
      macSeen = true;
      break;
    }
    if (offset > 1024) return "HeaderTooLarge";
  }
  if (!macSeen) return "MissingMac";
  if (offset > 1024) return "HeaderTooLarge";
  if (offset >= blob.length) return "NoPayload";
  if (lines[0] !== "age-encryption.org/v1") return "BadIntroLine";

  let tlockStanza = null;
  let tlockCount = 0;
  let greaseCount = 0;
  for (const line of lines) {
    if (!line.startsWith("-> ")) continue;
    const stanza = line.slice(3);
    const type = stanza.split(" ")[0];
    if (type === "tlock") {
      tlockCount += 1;
      tlockStanza = stanza;
    } else if (type.endsWith("-grease")) {
      greaseCount += 1;
    } else {
      // A foreign stanza would be a decryption path for someone other than the round.
      return "ForeignStanza";
    }
  }
  if (tlockCount === 0 && greaseCount === 0) return "NoStanza";
  if (tlockCount === 0) return "WrongStanzaType";
  if (tlockCount > 1) return "MultipleTlockStanzas";
  if (greaseCount > 1) return "MultipleGreaseStanzas";

  const args = tlockStanza.split(" ");
  if (args.length !== 3) return "MalformedStanzaArgs";
  const [, roundArg, chainArg] = args;
  if (roundArg.length === 0 || (roundArg.length > 1 && roundArg.startsWith("0"))) {
    return "NonCanonicalRound";
  }
  if (!/^[0-9]+$/.test(roundArg)) return "NonCanonicalRound";
  if (BigInt(roundArg) !== BigInt(expectedRound)) return "RoundMismatch";
  if (!/^[0-9a-f]{64}$/.test(chainArg) || chainArg !== expectedChain) return "ChainHashMismatch";

  for (const line of lines.slice(1)) {
    if (line.startsWith("-> ")) continue;
    let body = line.startsWith("---") ? line.slice(3) : line;
    if (body.startsWith(" ")) body = body.slice(1);
    if (body.length > 0 && !/^[A-Za-z0-9+/]+$/.test(body)) return "NonCanonicalBase64";
  }
  return "";
}

const [command, ...args] = process.argv.slice(2);

try {
  if (command === "encrypt") {
    const [round, plaintextHex] = args;
    const armored = await timelockEncrypt(
      Number(round),
      Buffer.from(plaintextHex, "hex"),
      offlineClient(null),
    );
    // De-armored on the way out: this is what a client must actually submit.
    console.log(dearmor(armored).toString("hex"));
  } else if (command === "decrypt") {
    const [blobHex, signatureHex] = args;
    const blob = Buffer.from(blobHex, "hex");
    // tlock-js logs the beacon it received to stdout. Silence it for the duration of the
    // call: this tool's stdout is parsed by the harness, so it carries the result and
    // nothing else.
    const log = console.log;
    console.log = () => {};
    let plaintext;
    try {
      plaintext = await timelockDecrypt(armor(blob), offlineClient(signatureHex));
    } finally {
      console.log = log;
    }
    console.log(Buffer.from(plaintext).toString("hex"));
  } else if (command === "profile") {
    const [blobHex, round, chain] = args;
    const verdict = validateProfile(Buffer.from(blobHex, "hex"), round, chain ?? CHAIN_HASH);
    if (verdict === "") {
      console.log("OK");
    } else {
      console.log(verdict);
      process.exit(2);
    }
  } else {
    console.error(`unknown command \`${command}\``);
    process.exit(1);
  }
} catch (error) {
  console.error(`error: ${error.message}`);
  process.exit(1);
}
