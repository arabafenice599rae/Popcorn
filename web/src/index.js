// The DOM-free half of the browser client, bundled on its own as `dist/popcorn.mjs`.
//
// It exists so the browser path can be tested without a browser: `test/browser-path.mjs`
// imports this bundle — the same bundling the page gets — builds transactions with it, and
// hands them to the Rust and Go tools to confirm they agree about the bytes, the identity
// and the profile.

export * from "./borsh.js";
export * from "./popcorn.js";
export * from "./actions.js";
export * from "./wallet.js";
export { NodeClient } from "./api.js";
