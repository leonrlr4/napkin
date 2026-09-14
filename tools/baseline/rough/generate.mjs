// Generates crates/rough/tests/baseline/*.json from roughjs@4.6.4 and the dependency
// versions Excalidraw's yarn.lock resolves at commit afa3a653fc5d2b742adcbd5a6063187b056d2419.
//
//   cd tools/baseline && npm ci && npm run rough

import { join } from "node:path";

import { REPO_ROOT, assertVersions, bundleAndImport, encode, resolveFrom, runCase, writeGroup } from "../lib/harness.mjs";
import { groups } from "./cases.mjs";

// Versions from Excalidraw's yarn.lock at the pinned commit. roughjs's own
// package.json only gives ranges (^0.5.2 ...), so a fresh install could drift.
assertVersions({
  roughjs: { pkg: "roughjs", from: null, version: "4.6.4" },
  "hachure-fill": { pkg: "hachure-fill", from: "roughjs", version: "0.5.2" },
  "path-data-parser": { pkg: "path-data-parser", from: "roughjs", version: "0.1.0" },
  "points-on-curve": { pkg: "points-on-curve", from: "roughjs", version: "0.2.0" },
  "points-on-path": { pkg: "points-on-path", from: "roughjs", version: "0.2.1" },
});

// Excalidraw imports roughjs/bin/* (the unminified tsc output), which Node cannot load
// directly because its internal imports omit file extensions; esbuild resolves them the
// same way Excalidraw's Vite build does. Dependencies are resolved from roughjs's own
// location so they are the copies roughjs runs with.
const lib = await bundleAndImport(
  "rough",
  `
  export { RoughGenerator } from ${JSON.stringify(resolveFrom("roughjs", "roughjs/bin/generator.js"))};
  export { Random } from ${JSON.stringify(resolveFrom("roughjs", "roughjs/bin/math.js"))};
  export { hachureLines } from ${JSON.stringify(resolveFrom("roughjs", "hachure-fill"))};
  export { parsePath, absolutize, normalize } from ${JSON.stringify(resolveFrom("roughjs", "path-data-parser"))};
  export { pointsOnBezierCurves, simplify } from ${JSON.stringify(resolveFrom("roughjs", "points-on-curve"))};
  export { curveToBezier } from ${JSON.stringify(resolveFrom("roughjs", "points-on-curve/lib/curve-to-bezier.js"))};
  export { pointsOnPath } from ${JSON.stringify(resolveFrom("roughjs", "points-on-path"))};
  `,
);

/**
 * Calls a RoughGenerator method on a fresh generator. rough.js's `_o(undefined)` returns the
 * generator's own defaultOptions object, and the first random draw then attaches a seed-0
 * randomizer to it that every later call copies, so one options-less call would turn the
 * rest of the run into Math.random. A fresh generator per case keeps cases independent.
 */
function generate(method, args) {
  return drawable(new lib.RoughGenerator()[method](...args));
}

/** The drawable as JSON, without the randomizer object rough.js leaves on its options. */
function drawable(d) {
  const { randomizer, ...options } = d.options;
  return { shape: d.shape, options, sets: d.sets.map(({ type, ops }) => ({ type, ops })) };
}

const calls = {
  atan2: (y, x) => Math.atan2(y, x),
  hypot: (x, y) => Math.hypot(x, y),
  Random: (seed, count) => {
    const random = new lib.Random(seed);
    return Array.from({ length: count }, () => random.next());
  },
  parsePath: (d) => lib.parsePath(d),
  absolutize: (d) => lib.absolutize(lib.parsePath(d)),
  normalize: (d) => lib.normalize(lib.absolutize(lib.parsePath(d))),
  pointsOnBezierCurves: (points, tolerance, distance) => lib.pointsOnBezierCurves(points, tolerance, distance),
  simplify: (points, distance) => lib.simplify(points, distance),
  curveToBezier: (points, curveTightness) => lib.curveToBezier(points, curveTightness),
  pointsOnPath: (d, tolerance, distance) => lib.pointsOnPath(d, tolerance, distance),
  // hachureLines rotates the caller's polygons in place and back again; the float drift
  // it leaves behind is observable (cross-hatch fills the same polygons twice).
  hachureLines: (polygons, gap, angle, stepOffset) => {
    const lines = lib.hachureLines(polygons, gap, angle, stepOffset);
    return { lines, polygons };
  },
  line: (...args) => generate("line", args),
  rectangle: (...args) => generate("rectangle", args),
  ellipse: (...args) => generate("ellipse", args),
  circle: (...args) => generate("circle", args),
  linearPath: (...args) => generate("linearPath", args),
  arc: (...args) => generate("arc", args),
  curve: (...args) => generate("curve", args),
  polygon: (...args) => generate("polygon", args),
  path: (...args) => generate("path", args),
};

const outDir = join(REPO_ROOT, "crates", "rough", "tests", "baseline");
const source = "roughjs@4.6.4 (hachure-fill@0.5.2, path-data-parser@0.1.0, points-on-curve@0.2.0, points-on-path@0.2.1)";
// `js_math` isn't roughjs at all: it's node's own `Math.atan2`/`Math.hypot` (V8), which
// `crates/rough/src/js.rs` ports for `crates/scene` bit-for-bit. Node's `process.version`
// carries the V8 build (`process.versions.v8`) that pins the exact fdlibm/Torque revision.
const jsMathSource = `node@${process.version.slice(1)} (V8 ${process.versions.v8}) Math.atan2/Math.hypot`;

/**
 * `encode` from harness.mjs, plus `-0`: the shared `encode` (and hence `JSON.stringify`,
 * which does not distinguish `-0` from `0`) is deliberately left alone here, since it is
 * shared with every other group and changing it risks silently changing baselines this fix
 * must leave byte-identical. `js_math` needs the distinction — `Math.atan2`'s sign of zero
 * is part of its contract (`atan2(-0, -0) === -Math.PI`, not `Math.PI`) and several of its
 * random/edge cases are exactly zero on one or both operands.
 */
function encodeSignedZero(value) {
  if (typeof value === "number" && Object.is(value, -0)) return "-0";
  return encode(value);
}

for (const [group, specs] of Object.entries(groups)) {
  if (group === "js_math") {
    // `Math.atan2`/`Math.hypot` never call `Math.random` or throw, so the generic
    // `runCase` (which exists for roughjs's Math.random reproducibility dance and its
    // `throws` capture) is unneeded ceremony here.
    const cases = specs.map(({ name, call, args }) => ({
      name,
      call,
      args: args.map(encodeSignedZero),
      compare: "exact",
      expected: encodeSignedZero(calls[call](...args)),
    }));
    writeGroup(outDir, group, jsMathSource, cases);
    continue;
  }
  const cases = specs.map(({ name, call, args }) => {
    // structuredClone: calls such as hachureLines mutate their arguments, and the
    // recorded args must be what the Rust side receives.
    const recordedArgs = structuredClone(args);
    const { compare, expected } = runCase(name, () => calls[call](...structuredClone(args)));
    return { name, call, args: encode(recordedArgs), compare, expected };
  });
  writeGroup(outDir, group, source, cases);
}
