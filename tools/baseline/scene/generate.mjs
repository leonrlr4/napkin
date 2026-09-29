// Generates crates/scene/tests/baseline/*.json by running Excalidraw's own source at the
// pinned commit (fetched into .cache/) on the inputs in cases.mjs.
//
//   cd tools/baseline && npm ci && npm run scene

import { join } from "node:path";

import { EXCALIDRAW_COMMIT, bundleExcalidraw } from "../lib/excalidraw.mjs";
import { REPO_ROOT, assertVersions, runCase, writeGroup } from "../lib/harness.mjs";
import { colors, duplicateCases, fractionalKeys, fractionalRanges, freedrawOutlineExtras, indexScenarios, newElementCalls, renderContexts, shapeElements, skeletonBatches, zindexCases } from "./cases.mjs";

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
  export { moveOneLeft, moveOneRight } from "@excalidraw/element/zindex";
  export { Scene } from "@excalidraw/element/Scene";
  export { applyDarkModeFilter, isTransparent, DEFAULT_GRID_SIZE } from "@excalidraw/common";
  export { generateKeyBetween, generateNKeysBetween } from "@excalidraw/fractional-indexing";
  export { convertToExcalidrawElements } from "@excalidraw/element/transform";
  export { duplicateElements } from "@excalidraw/element/duplicate";
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

/** `fa`/`fb`/`fbDel`: children of the `frame1` frame element (see `zindexElement`). */
const FRAME_CHILDREN = new Set(["fa", "fb", "fbDel"]);

/**
 * A complete `rectangle` (or `text` label, or `frame`) element for the zindex cases: `id` and
 * `index` (`a0`, `a1`, ... in array order) are the only fields that vary by case; `g1`/`g2` are
 * in group `"G"`, `t` is `r`'s bound label, `del`/`fbDel` are soft-deleted, `frame1` is a frame
 * element, and `fa`/`fb`/`fbDel` are its children.
 */
function zindexElement(id, position) {
  const el = {
    id, type: "rectangle", x: 0, y: 0, width: 10, height: 10, angle: 0,
    strokeColor: "#1e1e1e", backgroundColor: "transparent", fillStyle: "solid",
    strokeWidth: 2, strokeStyle: "solid", roughness: 1, opacity: 100,
    groupIds: id === "g1" || id === "g2" ? ["G"] : [],
    frameId: FRAME_CHILDREN.has(id) ? "frame1" : null, index: `a${position}`,
    roundness: null, seed: 1, version: 1, versionNonce: 0,
    isDeleted: id === "del" || id === "fbDel",
    boundElements: id === "r" ? [{ id: "t", type: "text" }] : null, updated: 1,
  };
  if (id === "t") {
    Object.assign(el, {
      type: "text", containerId: "r", text: "hi", fontSize: 20, baseFontSize: 20,
      fontFamily: 5, textAlign: "center", verticalAlign: "middle", originalText: "hi",
      autoResize: true, lineHeight: 1.25, boundElements: null,
    });
  }
  if (id === "frame1") {
    el.type = "frame";
  }
  return el;
}

writeGroup(
  outDir,
  "zindex",
  source,
  zindexCases.map(([name, ids, selected, direction]) => {
    const fn = direction === "right" ? "moveOneRight" : "moveOneLeft";
    const appState = {
      selectedElementIds: Object.fromEntries(selected.map((id) => [id, true])),
      editingGroupId: null,
    };
    return {
      name,
      call: fn,
      args: [ids, selected, direction],
      ...runCase(name, () => {
        const elements = ids.map((id, position) => zindexElement(id, position));
        const scene = new lib.Scene(elements);
        return lib[fn](elements, appState, scene).map(({ id, index, version }) => ({ id, index, version }));
      }),
    };
  }),
);

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
 * Stable across Math.random streams: `seed` and `versionNonce` are dropped, every id is
 * renamed to `e<position>` in output order (references included), and every `groupId` is
 * renamed to `g<n>` (1-based) in first-occurrence order, scanning each element's own
 * `groupIds` in the same output-order pass. `crates/scene/tests/baseline.rs`'s
 * `normalize_skeleton_output` does the same; shared by the `skeleton` and `duplicate` groups.
 */
function normalizeSkeletonOutput(elements) {
  const rename = new Map(elements.map((e, i) => [e.id, `e${i}`]));
  const id = (value) => rename.get(value) ?? value;
  const groupRename = new Map();
  const groupId = (value) => {
    if (!groupRename.has(value)) groupRename.set(value, `g${groupRename.size + 1}`);
    return groupRename.get(value);
  };
  return elements.map(({ seed, versionNonce, ...rest }) => {
    const out = structuredClone(rest);
    out.id = id(out.id);
    if (typeof out.containerId === "string") out.containerId = id(out.containerId);
    if (Array.isArray(out.boundElements)) out.boundElements = out.boundElements.map((b) => ({ ...b, id: id(b.id) }));
    for (const key of ["startBinding", "endBinding"]) {
      if (out[key]) out[key] = { ...out[key], elementId: id(out[key].elementId) };
    }
    if (Array.isArray(out.groupIds)) out.groupIds = out.groupIds.map(groupId);
    return out;
  });
}

/**
 * `appState.selectedGroupIds` as forming the selection through the editor would leave it: a
 * group counts as selected exactly when every one of its members (at any nesting level) is in
 * `ids` (mirrors `crates::duplicate::selected_group_ids`).
 */
function selectedGroupIdsFor(elements, ids) {
  const selected = new Set(ids);
  const candidates = new Set();
  for (const element of elements) {
    if (selected.has(element.id)) {
      for (const groupId of element.groupIds ?? []) candidates.add(groupId);
    }
  }
  const result = {};
  for (const groupId of candidates) {
    const fullySelected = elements.every(
      (element) => !(element.groupIds ?? []).includes(groupId) || selected.has(element.id),
    );
    if (fullySelected) result[groupId] = true;
  }
  return result;
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

writeGroup(
  outDir,
  "duplicate",
  source,
  duplicateCases.map(([name, elements, ids, mode]) => ({
    name,
    call: "duplicateElements",
    args: [elements, ids ?? [], mode],
    ...runCase(
      name,
      () => {
        const clonedElements = structuredClone(elements);
        const opts =
          mode === "everything"
            ? { type: "everything", elements: clonedElements, randomizeSeed: true }
            : {
                type: "in-place",
                elements: clonedElements,
                idsOfElementsToDuplicate: new Map(
                  clonedElements.filter((el) => ids.includes(el.id)).map((el) => [el.id, el]),
                ),
                appState: {
                  editingGroupId: null,
                  selectedGroupIds: selectedGroupIdsFor(clonedElements, ids),
                },
                randomizeSeed: true,
                overrides: ({ origElement }) => ({
                  x: origElement.x + lib.DEFAULT_GRID_SIZE / 2,
                  y: origElement.y + lib.DEFAULT_GRID_SIZE / 2,
                }),
              };
        const { elementsWithDuplicates, duplicatedElements } = lib.duplicateElements(opts);
        const result = mode === "everything" ? duplicatedElements : elementsWithDuplicates;
        const synced = lib.syncMovedIndices(result, new Map(duplicatedElements.map((e) => [e.id, e])));
        return normalizeSkeletonOutput(synced);
      },
      { exactDespiteRandom: true },
    ),
  })),
);
