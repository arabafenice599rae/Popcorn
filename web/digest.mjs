// The digest of everything that goes into the bundle.
//
// `dist/` is committed so that building the node needs nothing but cargo — an operator should
// not have to install npm to serve a page. That is only safe if a source change without a
// rebuild is detectable, and the bundler is not guaranteed to be byte-reproducible across
// versions, so the check is on the inputs rather than the output.
//
//   node digest.mjs           -> print the digest
//   node digest.mjs --check   -> compare it with dist/build.json, exit 1 on a mismatch
//   node digest.mjs --write   -> (re)write dist/build.json

import { createHash } from "node:crypto";
import { readFileSync, readdirSync, writeFileSync } from "node:fs";

const SOURCES = [
  ...readdirSync("src").sort().map((name) => `src/${name}`),
  "public/index.html",
  "public/app.css",
  "package-lock.json",
];

const files = Object.fromEntries(SOURCES.map((path) => [
  path,
  createHash("sha256").update(readFileSync(path)).digest("hex"),
]));

const combined = createHash("sha256");
for (const [path, digest] of Object.entries(files)) combined.update(`${path} ${digest}\n`);
const digest = combined.digest("hex");

const mode = process.argv[2];
if (mode === "--write") {
  writeFileSync("dist/build.json", `${JSON.stringify({ sources: files, digest }, null, 2)}\n`);
  console.log(digest);
} else if (mode === "--check") {
  let recorded;
  try {
    recorded = JSON.parse(readFileSync("dist/build.json", "utf8"));
  } catch {
    console.error("dist/build.json is missing: run web/build.sh and commit the result");
    process.exit(1);
  }
  if (recorded.digest !== digest) {
    console.error("the committed bundle is stale: web/ sources changed without a rebuild");
    for (const [path, value] of Object.entries(files)) {
      if (recorded.sources?.[path] !== value) console.error(`  changed: ${path}`);
    }
    for (const path of Object.keys(recorded.sources ?? {})) {
      if (!(path in files)) console.error(`  removed: ${path}`);
    }
    console.error("run web/build.sh and commit dist/");
    process.exit(1);
  }
  console.log(digest);
} else {
  console.log(digest);
}
