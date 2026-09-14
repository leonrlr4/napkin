// Fetches Excalidraw at the commit spec §3 pins and bundles its own element/common sources,
// so the baseline runs exactly the code excalidraw.com runs instead of hand-copied excerpts.

import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync } from "node:fs";
import { join } from "node:path";

import { BASELINE_DIR, bundleAndImport } from "./harness.mjs";

export const EXCALIDRAW_COMMIT = "afa3a653fc5d2b742adcbd5a6063187b056d2419";

function git(cwd, ...args) {
  return execFileSync("git", args, { cwd, encoding: "utf8", stdio: ["ignore", "pipe", "inherit"] }).trim();
}

/** A shallow checkout of EXCALIDRAW_COMMIT under .cache/, reused when already present. */
export function excalidrawCheckout() {
  const dir = join(BASELINE_DIR, ".cache", `excalidraw-${EXCALIDRAW_COMMIT}`);
  if (!existsSync(join(dir, ".git"))) {
    mkdirSync(dir, { recursive: true });
    git(dir, "init", "--quiet");
    git(dir, "remote", "add", "origin", "https://github.com/excalidraw/excalidraw.git");
    git(dir, "fetch", "--quiet", "--depth", "1", "origin", EXCALIDRAW_COMMIT);
    git(dir, "checkout", "--quiet", "FETCH_HEAD");
  }
  const head = git(dir, "rev-parse", "HEAD");
  if (head !== EXCALIDRAW_COMMIT) {
    throw new Error(`${dir} is at ${head}, expected ${EXCALIDRAW_COMMIT}; delete it and rerun`);
  }
  return dir;
}

// Imported by Excalidraw modules that the shape code never calls; bundling them would
// need their npm packages installed for nothing. The default export stays callable because
// Scene.ts wraps a function with lodash.throttle at module load; whatever it returns throws
// if the baseline ever reaches it.
const STUBBED = /^(@braintree\/sanitize-url|es6-promise-pool|lodash\.throttle|nanoid)$/;

const stubPlugin = {
  name: "stub-unused",
  setup(build) {
    build.onResolve({ filter: STUBBED }, (args) => ({ path: args.path, namespace: "stub" }));
    build.onLoad({ filter: /.*/, namespace: "stub" }, () => ({
      contents: [
        "const unused = () => { throw new Error('stubbed module called'); };",
        "export default () => unused; export const sanitizeUrl = unused; export const nanoid = unused;",
      ].join("\n"),
      loader: "js",
    }));
  },
};

/** Bundles `entrySource` against the checkout's tsconfig path aliases and imports it. */
export function bundleExcalidraw(name, entrySource) {
  const checkout = excalidrawCheckout();
  return bundleAndImport(name, entrySource, {
    tsconfig: join(checkout, "tsconfig.json"),
    nodePaths: [join(BASELINE_DIR, "node_modules")],
    plugins: [stubPlugin],
    define: { "import.meta.env": "{}" },
    loader: { ".woff2": "empty", ".png": "empty", ".svg": "empty", ".scss": "empty", ".css": "empty" },
  });
}
