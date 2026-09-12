// A thin client over the node API of SPEC.md §9.1.
//
// Same origin by default: the node serves this page itself, so there is no CORS surface and
// no configured endpoint to get wrong. `base` exists for the headless test, which runs the
// same code outside a browser.

export class NodeClient {
  constructor(base = "") {
    this.base = base.replace(/\/$/, "");
  }

  async get(path) {
    const response = await fetch(this.base + path, { headers: { accept: "application/json" } });
    const body = await response.json().catch(() => ({}));
    if (!response.ok) {
      const error = new Error(body.error ?? `${response.status} ${response.statusText}`);
      error.status = response.status;
      throw error;
    }
    return body;
  }

  async post(path, payload) {
    const response = await fetch(this.base + path, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(payload),
    });
    const body = await response.json().catch(() => ({}));
    if (!response.ok) throw new Error(body.error ?? `${response.status} ${response.statusText}`);
    return body;
  }

  head() { return this.get("/head"); }
  block(height) { return this.get(`/block/${height}`); }
  account(id) { return this.get(`/account/${id}`); }
  pair(id) { return this.get(`/pair/${id}`); }
  tokens() { return this.get("/tokens"); }
  pairs() { return this.get("/pairs"); }
  supply() { return this.get("/supply"); }
  params() { return this.get("/params"); }
  blob(hash) { return this.get(`/blob/${hash}`); }
  topic(id, from = 1) { return this.get(`/topic/${id}?from=${from}`); }
  exportFrom(height) { return this.get(`/chain/export?from=${height}`); }

  /** Submit a blind blob and receive the signed receipt of §9.2. */
  submit(blobBase64, targetRound) {
    return this.post("/tx", { blob: blobBase64, target_round: Number(targetRound) });
  }

  /**
   * Subscribe to produced blocks. The socket is a convenience: every field it carries is
   * also derivable from `/chain/export`, which is what a verifier uses.
   */
  stream(onBlock, onState) {
    const origin = this.base || globalThis.location?.origin || "";
    const url = origin.replace(/^http/, "ws") + "/stream";
    let socket;
    let closed = false;
    let backoff = 1000;

    const connect = () => {
      if (closed) return;
      socket = new WebSocket(url);
      socket.addEventListener("open", () => {
        backoff = 1000;
        onState?.("live");
      });
      socket.addEventListener("message", (event) => {
        try {
          onBlock(JSON.parse(event.data));
        } catch {
          // A frame we cannot parse is not a reason to drop the subscription.
        }
      });
      socket.addEventListener("close", () => {
        if (closed) return;
        onState?.("reconnecting");
        setTimeout(connect, backoff);
        backoff = Math.min(backoff * 2, 15000);
      });
      socket.addEventListener("error", () => socket.close());
    };

    connect();
    return () => {
      closed = true;
      socket?.close();
    };
  }
}
