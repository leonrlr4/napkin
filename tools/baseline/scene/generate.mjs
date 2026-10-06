// Generates crates/scene/tests/baseline/*.json by running Excalidraw's own source at the
// pinned commit (fetched into .cache/) on the inputs in cases.mjs.
//
//   cd tools/baseline && npm ci && npm run scene

import { join } from "node:path";

import { EXCALIDRAW_COMMIT, bundleExcalidraw } from "../lib/excalidraw.mjs";
import { REPO_ROOT, assertVersions, runCase, writeGroup } from "../lib/harness.mjs";
import { colors, fractionalKeys, fractionalRanges, freedrawOutlineExtras, indexScenarios, newElementCalls, renderContexts, shapeElements, skeletonBatches } from "./cases.mjs";

// Versions from Excalidraw's yarn.lock at the pinned commit.
assertVersions({
  roughjs: { pkg: "roughjs", from: null, version: "4.6.4" },
  "perfect-freehand": { pkg: "perfect-freehand", from: null, version: "1.2.0" },
  "points-on-curve": { pkg: "points-on-curve", from: null, version: "1.0.1" },
  tinycolor2: { pkg: "tinycolor2", from: null, version: "1.6.0" },
});

const lib = await bundleExcalidraw(
  "scene",
  `
  export { ShapeCache, generateRoughOptions, getFreedrawOutlinePoints } from "@excalidraw/element/shape";
  export { newElement, newLinearElement, newArrowElement, newFreeDrawElement, newTextElement } from "@excalidraw/element/newElement";
  export { setCustomTextMetricsProvider } from "@excalidraw/element/textMeasurements";
  export { syncMovedIndices, syncInvalidIndices } from "@excalidraw/element/fractionalIndex";
  export { applyDarkModeFilter, isTransparent } from "@excalidraw/common";
  export { generateKeyBetween, generateNKeysBetween } from "@excalidraw/fractional-indexing";
  export { convertToExcalidrawElements } from "@excalidraw/element/transform";
  `,
);

const outDir = join(REPO_ROOT, "crates", "scene", "tests", "baseline");
const source = `excalidraw@${EXCALIDRAW_COMMIT} (roughjs@4.6.4, perfect-freehand@1.2.0, tinycolor2@1.6.0)`;

function drawable(d) {
  const { randomizer, ...options } = d.options;
  return { shape: d.shape, options, sets: d.sets.map(({ type, ops }) => ({ type, ops })) };
}

/**
 * The freedraw stroke is an SVG path string built by getSvgPathFromStroke:
 * "M x,y Q x,y x,y ... L x,y Z", numbers already cut by its TO_FIXED_PRECISION regex.
 * Recorded as ops so the Rust side compares numbers, not JS number formatting.
 */
function svgPathOps(path) {
  const ops = [];
  let command = null;
  let pending = [];
  for (const token of path.split(" ").filter(Boolean)) {
    if (/^[A-Z]$/.test(token)) {
      command = token;
      if (command === "Z") ops.push({ op: "close", data: [] });
      continue;
    }
    pending.push(...token.split(",").map(Number));
    if (command === "M" && pending.length === 2) {
      ops.push({ op: "move", data: pending });
      pending = [];
    } else if (command === "L" && pending.length === 2) {
      ops.push({ op: "line", data: pending });
      pending = [];
    } else if (command === "Q" && pending.length === 4) {
      ops.push({ op: "quad", data: pending });
      pending = [];
    }
  }
  if (pending.length) throw new Error(`dangling numbers in ${path}`);
  return ops;
}

function shapeJson(shape) {
  if (shape === null) return null;
  if (Array.isArray(shape)) {
    return shape.map((s) => (typeof s === "string" ? { svgPath: svgPathOps(s) } : drawable(s)));
  }
  return drawable(shape);
}

const shapes = [];
const roughOptions = [];
for (const { label, element } of shapeElements) {
  for (const context of renderContexts) {
    const renderConfig = {
      isExporting: true,
      canvasBackgroundColor: context.canvasBackgroundColor,
      embedsValidationStatus: null,
      theme: context.theme,
    };
    const args = [structuredClone(element), { theme: context.theme, canvasBackgroundColor: context.canvasBackgroundColor }];
    const name = `${label}/${context.label}`;
    shapes.push({
      name,
      call: "generateElementShape",
      args,
      ...runCase(name, () => shapeJson(lib.ShapeCache.generateElementShape(structuredClone(element), renderConfig))),
    });
  }
  if (["rectangle", "diamond", "ellipse", "line", "arrow", "freedraw"].includes(element.type)) {
    // continuousPath only sets preserveVertices and dark mode only maps colors, so two
    // combinations cover both flags.
    for (const [continuousPath, isDarkMode] of [[false, false], [true, true]]) {
      const name = `${label}/continuous${continuousPath}/dark${isDarkMode}`;
      roughOptions.push({
        name,
        call: "generateRoughOptions",
        args: [structuredClone(element), continuousPath, isDarkMode],
        ...runCase(name, () => lib.generateRoughOptions(structuredClone(element), continuousPath, isDarkMode)),
      });
    }
  }
}
const FAMILY = { rectangle: "generic", diamond: "generic", ellipse: "generic", line: "linear", arrow: "linear", freedraw: "freedraw" };
for (const family of ["generic", "linear", "freedraw", "other"]) {
  writeGroup(outDir, `shapes_${family}`, source, shapes.filter((c) => (FAMILY[c.args[0].type] ?? "other") === family));
}
writeGroup(outDir, "rough_options", source, roughOptions);

writeGroup(
  outDir,
  "freedraw_outline",
  source,
  shapeElements
    .filter(({ element }) => element.type === "freedraw")
    .concat(freedrawOutlineExtras)
    .map(({ label, element }) => ({
      name: label,
      call: "getFreedrawOutlinePoints",
      args: [element],
      ...runCase(label, () => lib.getFreedrawOutlinePoints(structuredClone(element))),
    })),
);

writeGroup(
  outDir,
  "colors",
  source,
  colors.flatMap((color) => [
    { name: `dark/${JSON.stringify(color)}`, call: "applyDarkModeFilter", args: [color], ...runCase(color, () => lib.applyDarkModeFilter(color)) },
    { name: `transparent/${JSON.stringify(color)}`, call: "isTransparent", args: [color], ...runCase(color, () => lib.isTransparent(color)) },
  ]),
);

const indexCases = [
  ...fractionalKeys.map(([a, b]) => {
    const name = `between/${a}/${b}`;
    return { name, call: "generateKeyBetween", args: [a, b], ...runCase(name, () => lib.generateKeyBetween(a, b)) };
  }),
  ...fractionalRanges.map(([a, b, n]) => {
    const name = `nBetween/${a}/${b}/${n}`;
    return { name, call: "generateNKeysBetween", args: [a, b, n], ...runCase(name, () => lib.generateNKeysBetween(a, b, n)) };
  }),
];
for (const [label, indices, moved] of indexScenarios) {
  const elements = () =>
    indices.map((index, i) => ({ id: `e${i}`, type: "rectangle", index, version: 1, versionNonce: 0, updated: 1 }));
  const summary = (els) => els.map(({ id, index, version }) => ({ id, index, version }));
  const movedName = `syncMoved/${label}`;
  indexCases.push({
    name: movedName,
    call: "syncMovedIndices",
    args: [indices, moved],
    ...runCase(movedName, () => {
      const els = elements();
      const movedMap = new Map(moved.map((i) => [els[i].id, els[i]]));
      return summary(lib.syncMovedIndices(els, movedMap));
    }),
  });
  const invalidName = `syncInvalid/${label}`;
  indexCases.push({
    name: invalidName,
    call: "syncInvalidIndices",
    args: [indices],
    ...runCase(invalidName, () => summary(lib.syncInvalidIndices(elements()))),
  });
}
writeGroup(outDir, "fractional_index", source, indexCases);

// Canvas text metrics do not exist in node; every UTF-16 code unit is 0.6em wide. The Rust
// side uses the same formula (`scene::sample::CharWidthMeasure`).
lib.setCustomTextMetricsProvider({ getLineWidth: (text, font) => text.length * parseFloat(font) * 0.6 });

// newElement stamps `updated`/`created` with Date.now(); pin it so the defaults are comparable.
// Callers pass id and seed, the two values that are random by design.
Date.now = () => 1;
writeGroup(
  outDir,
  "new_element",
  source,
  newElementCalls.map(([name, fn, opts]) => ({
    name,
    call: fn,
    args: [opts],
    ...runCase(name, () => lib[fn](structuredClone(opts))),
  })),
);

/**
 * Stable across Math.random streams: `seed` and `versionNonce` are dropped and every id is
 * renamed to `e<position>` in output order, references included. `crates/scene/tests/
 * baseline.rs`'s `normalize_skeleton_output` does the same.
 */
function normalizeSkeletonOutput(elements) {
  const rename = new Map(elements.map((e, i) => [e.id, `e${i}`]));
  const id = (value) => rename.get(value) ?? value;
  return elements.map(({ seed, versionNonce, ...rest }) => {
    const out = structuredClone(rest);
    out.id = id(out.id);
    if (typeof out.containerId === "string") out.containerId = id(out.containerId);
    if (Array.isArray(out.boundElements)) out.boundElements = out.boundElements.map((b) => ({ ...b, id: id(b.id) }));
    for (const key of ["startBinding", "endBinding"]) {
      if (out[key]) out[key] = { ...out[key], elementId: id(out[key].elementId) };
    }
    return out;
  });
}

writeGroup(
  outDir,
  "skeleton",
  source,
  skeletonBatches.map(([name, skeletons]) => ({
    name,
    call: "convertToExcalidrawElements",
    args: [skeletons],
    ...runCase(
      name,
      () => normalizeSkeletonOutput(lib.convertToExcalidrawElements(structuredClone(skeletons))),
      { exactDespiteRandom: true },
    ),
  })),
);
