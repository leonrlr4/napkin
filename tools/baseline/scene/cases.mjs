// Inputs for the scene baselines. Elements are complete objects shaped like Excalidraw's
// `newElement` output at the pinned commit, so the shape code sees what a saved file holds.

let nextId = 0;

/** `_newElementBase` defaults (packages/element/src/newElement.ts) plus overrides. */
function element(type, overrides = {}) {
  nextId += 1;
  return {
    id: `el${nextId}`,
    type,
    x: 0,
    y: 0,
    width: 120,
    height: 80,
    angle: 0,
    strokeColor: "#1e1e1e",
    backgroundColor: "transparent",
    fillStyle: "solid",
    strokeWidth: 2,
    strokeStyle: "solid",
    roughness: 1,
    opacity: 100,
    groupIds: [],
    frameId: null,
    index: "a0",
    roundness: null,
    seed: 1968410350,
    version: 1,
    versionNonce: 0,
    isDeleted: false,
    boundElements: null,
    updated: 1,
    created: 1,
    link: null,
    locked: false,
    ...overrides,
  };
}

function linear(type, points, overrides = {}) {
  const xs = points.map((p) => p[0]);
  const ys = points.map((p) => p[1]);
  return element(type, {
    width: points.length ? Math.max(...xs) - Math.min(...xs) : 0,
    height: points.length ? Math.max(...ys) - Math.min(...ys) : 0,
    points,
    startBinding: null,
    endBinding: null,
    startArrowhead: null,
    endArrowhead: null,
    ...(type === "line" ? { polygon: false } : { elbowed: false }),
    ...overrides,
  });
}

function freedraw(points, overrides = {}) {
  const xs = points.map((p) => p[0]);
  const ys = points.map((p) => p[1]);
  return element("freedraw", {
    width: points.length ? Math.max(...xs) - Math.min(...xs) : 0,
    height: points.length ? Math.max(...ys) - Math.min(...ys) : 0,
    points,
    pressures: [],
    simulatePressure: true,
    strokeOptions: { variability: "constant", streamline: 0.5 },
    ...overrides,
  });
}

const SEEDS = [1968410350, 7];
const SIZES = { normal: [120, 80], small: [30, 12], tiny: [8, 6], odd: [101, 57] };
const ROUNDNESS = {
  sharp: null,
  legacy: { type: 1 },
  proportional: { type: 2 },
  adaptive: { type: 3 },
  adaptiveValue: { type: 3, value: 12 },
};

/** One case per variant of each dimension, so every rule branch runs without a full cross product. */
function genericVariants(type) {
  const cases = [];
  const add = (label, overrides) => cases.push({ label: `${type}/${label}`, element: element(type, overrides) });
  for (const seed of SEEDS) {
    for (const roughness of [0, 1, 2]) add(`seed${seed}/r${roughness}`, { seed, roughness });
  }
  for (const [label, [width, height]] of Object.entries(SIZES)) add(`size/${label}`, { width, height });
  for (const [label, roundness] of Object.entries(ROUNDNESS)) {
    add(`roundness/${label}`, { roundness });
    add(`roundness/${label}/small`, { roundness, width: 18, height: 16 });
  }
  for (const fillStyle of ["hachure", "cross-hatch", "solid", "zigzag"]) {
    add(`fill/${fillStyle}`, { fillStyle, backgroundColor: "#ffc9c9" });
    add(`fill/${fillStyle}/r2/rounded`, { fillStyle, backgroundColor: "#a5d8ff", roughness: 2, roundness: ROUNDNESS.adaptive });
  }
  add("fill/transparentRgba", { fillStyle: "hachure", backgroundColor: "rgba(255, 0, 0, 0)" });
  for (const strokeStyle of ["dashed", "dotted"]) {
    add(`stroke/${strokeStyle}`, { strokeStyle });
    add(`stroke/${strokeStyle}/w4`, { strokeStyle, strokeWidth: 4, backgroundColor: "#b2f2bb", fillStyle: "hachure" });
  }
  add("strokeWidth1", { strokeWidth: 1, backgroundColor: "#ffec99", fillStyle: "hachure" });
  add("zeroSize", { width: 0, height: 0, roundness: ROUNDNESS.adaptive });
  return cases;
}

const ARROWHEADS = [
  "arrow", "bar", "circle", "circle_outline", "triangle", "triangle_outline", "diamond", "diamond_outline",
  "cardinality_one", "cardinality_many", "cardinality_one_or_many", "cardinality_exactly_one",
  "cardinality_zero_or_one", "cardinality_zero_or_many", "dot",
];

const TWO = [[0, 0], [160, 60]];
const ZIGZAG = [[0, 0], [60, 80], [140, 20], [200, 90]];
const SHORT_LAST = [[0, 0], [150, 0], [156, 4]];
const ELBOW = [[0, 0], [80, 0], [80, 120], [200, 120]];
const LOOP = [[0, 0], [100, 10], [60, 90], [3, 4]];

function linearVariants() {
  const cases = [];
  const add = (label, el) => cases.push({ label, element: el });
  for (const seed of SEEDS) {
    for (const roughness of [0, 1, 2]) {
      add(`line/sharp/seed${seed}/r${roughness}`, linear("line", ZIGZAG, { seed, roughness }));
      add(`line/round/seed${seed}/r${roughness}`, linear("line", ZIGZAG, { seed, roughness, roundness: ROUNDNESS.proportional }));
    }
  }
  add("line/two", linear("line", TWO));
  add("line/empty", linear("line", []));
  add("line/single", linear("line", [[0, 0]]));
  add("line/short", linear("line", [[0, 0], [20, 5]]));
  add("line/loop/transparent", linear("line", LOOP, { backgroundColor: "transparent" }));
  for (const fillStyle of ["hachure", "cross-hatch", "solid", "zigzag"]) {
    add(`line/loop/${fillStyle}`, linear("line", LOOP, { backgroundColor: "#ffc9c9", fillStyle }));
    add(`line/loop/${fillStyle}/round`, linear("line", LOOP, { backgroundColor: "#ffc9c9", fillStyle, roundness: ROUNDNESS.proportional }));
  }
  add("line/polygon", linear("line", [...LOOP.slice(0, 3), [0, 0]], { polygon: true, backgroundColor: "#ffc9c9", fillStyle: "solid" }));
  add("line/dotted", linear("line", ZIGZAG, { strokeStyle: "dotted" }));

  for (const head of ARROWHEADS) {
    add(`arrow/end/${head}`, linear("arrow", TWO, { endArrowhead: head }));
    add(`arrow/start/${head}/round`, linear("arrow", ZIGZAG, { startArrowhead: head, roundness: ROUNDNESS.proportional }));
    add(`arrow/both/${head}/dotted`, linear("arrow", SHORT_LAST, { startArrowhead: head, endArrowhead: head, strokeStyle: "dotted", strokeWidth: 1 }));
  }
  for (const roughness of [0, 2]) {
    add(`arrow/r${roughness}`, linear("arrow", ZIGZAG, { endArrowhead: "triangle", roughness }));
  }
  add("arrow/dashed", linear("arrow", ZIGZAG, { endArrowhead: "arrow", strokeStyle: "dashed" }));
  add("arrow/endArrowheadMissing", (() => {
    const el = linear("arrow", TWO);
    delete el.endArrowhead;
    return el;
  })());
  add("arrow/single", linear("arrow", [[0, 0]], { endArrowhead: "arrow" }));
  add("arrow/empty", linear("arrow", [], { endArrowhead: "arrow" }));
  add("arrow/elbow", linear("arrow", ELBOW, { elbowed: true, endArrowhead: "arrow", fixedSegments: null, startIsSpecial: null, endIsSpecial: null }));
  add("arrow/elbow/short", linear("arrow", [[0, 0], [10, 0], [10, 6], [40, 6]], { elbowed: true, startArrowhead: "circle_outline", endArrowhead: "triangle" }));
  add("arrow/elbow/extreme", linear("arrow", [[0, 0], [2e6, 0]], { elbowed: true, endArrowhead: "arrow" }));
  return cases;
}

function spiral(n, step = 6) {
  return Array.from({ length: n }, (_, i) => {
    const t = i / 4;
    return [Math.round((10 + 3 * t) * Math.cos(t) * 100) / 100 + step, Math.round((10 + 3 * t) * Math.sin(t) * 100) / 100];
  });
}

function freedrawVariants() {
  const cases = [];
  const add = (label, el) => cases.push({ label, element: el });
  const STROKES = { empty: [], one: [[0, 0]], two: [[0, 0], [12, 7]], three: [[0, 0], [12, 7], [30, 2]], spiral: spiral(40) };
  for (const variability of ["constant", "variable"]) {
    for (const [label, points] of Object.entries(STROKES)) {
      add(`freedraw/${variability}/${label}`, freedraw(points, { strokeOptions: { variability, streamline: 0.5 } }));
    }
    add(`freedraw/${variability}/duplicates`, freedraw([[0, 0], [0, 0], [5, 5], [5, 5], [5, 5], [20, 0]], { strokeOptions: { variability, streamline: 0.5 } }));
    add(`freedraw/${variability}/streamline0.2`, freedraw(spiral(25), { strokeOptions: { variability, streamline: 0.2 } }));
    add(`freedraw/${variability}/width4`, freedraw(spiral(25), { strokeWidth: 4, strokeOptions: { variability, streamline: 0.5 } }));
    add(`freedraw/${variability}/loopFill`, freedraw([...spiral(12), [6 + 10, 0]], { backgroundColor: "#ffc9c9", fillStyle: "hachure", strokeOptions: { variability, streamline: 0.5 } }));
    add(`freedraw/${variability}/sharpTurn`, freedraw([[0, 0], [40, 0], [80, 0], [40, 2], [0, 4]], { strokeOptions: { variability, streamline: 0.5 } }));
  }
  const pressurePoints = spiral(20);
  add("freedraw/variable/realPressure", freedraw(pressurePoints, {
    simulatePressure: false,
    pressures: pressurePoints.map((_, i) => 0.2 + (i % 7) / 10),
    strokeOptions: { variability: "variable", streamline: 0.5 },
  }));
  add("freedraw/variable/shortPressures", freedraw(pressurePoints, {
    simulatePressure: false,
    pressures: [0.3, 0.9],
    strokeOptions: { variability: "variable", streamline: 0.5 },
  }));
  add("freedraw/strokeOptionsMissing", (() => {
    const el = freedraw(spiral(10));
    delete el.strokeOptions;
    return el;
  })());
  add("freedraw/tinyNumbers", freedraw([[0, 0], [1e-7, 3e-7], [0.0000015, 2]], { strokeOptions: { variability: "constant", streamline: 0 } }));
  // A single point draws a circle of radius strokeWidth * 1.4 = 2.8 starting at x + 2.8, so
  // the first outline x is about 1.5e-7: JS prints it in exponent form, and the path's
  // TO_FIXED_PRECISION regex then drops the exponent, turning it into 1.49.
  add("freedraw/exponentQuirk", freedraw([[-2.79999985, 1.2e-7]], { strokeOptions: { variability: "constant", streamline: 0.5 } }));
  return cases;
}

export const shapeElements = [
  ...genericVariants("rectangle"),
  ...genericVariants("diamond"),
  ...genericVariants("ellipse"),
  ...linearVariants(),
  ...freedrawVariants(),
  { label: "text", element: element("text", { text: "hi", fontSize: 20, fontFamily: 5, textAlign: "left", verticalAlign: "top", containerId: null, originalText: "hi", autoResize: true, lineHeight: 1.25 }) },
  { label: "image", element: element("image", { fileId: null, status: "pending", scale: [1, 1], crop: null }) },
  { label: "frame", element: element("frame", { name: null }) },
];

/**
 * Extra `freedraw_outline` cases that are not also `shapes_freedraw`/`rough_options` cases:
 * adding these to `shapeElements` would shift every `elN` id assigned after them, changing
 * unrelated baselines. `generate.mjs` appends these to the `shapeElements`-derived list only
 * for `getFreedrawOutlinePoints`.
 */
export const freedrawOutlineExtras = [
  // V8's `Math.atan2(-2.5, -1)` (the laser-pointer corner angle for this two-point stroke)
  // and glibc's `atan2` disagree in the last bit, which can flip which branch of
  // `getStrokeOutline`'s `theta <= Math.PI + pAngle` loops runs an extra step and change
  // the outline's point count.
  { label: "freedraw/constant/atan2LastBit", element: freedraw([[0, 0], [-2, -5]], { strokeOptions: { variability: "constant", streamline: 0.5 } }) },
];

/** Render contexts: dark mode flips colors; the canvas background feeds outline arrowheads. */
export const renderContexts = [
  { label: "light", theme: "light", canvasBackgroundColor: "#ffffff" },
  { label: "dark", theme: "dark", canvasBackgroundColor: "#ffffff" },
  { label: "lightTinted", theme: "light", canvasBackgroundColor: "#fffce8" },
];

export const colors = [
  "#1e1e1e", "#ffffff", "#000", "#fff", "#e03131", "#ffc9c9", "#a5d8ff80", "#a5d8ff00", "#abcd", "e03131", "#FFC9C9",
  "transparent", "TRANSPARENT", " #e03131 ", "red", "RebeccaPurple", "white", "black",
  "rgb(255, 0, 0)", "rgba(0, 128, 255, 0.5)", "rgba(0,0,0,0)", "rgb(100%, 50%, 0%)", "rgb 10 20 30", "rgb(300, -5, 1.5)",
  "hsl(120, 100%, 50%)", "hsla(240, 50%, 50%, 0.25)", "hsv(0, 100%, 100%)", "hsva(60 1 1 .5)",
  "hsl(0, 0.000000001%, 10.0%)", "hsl(0 0.00000000001 0.398)",
  "rgba(10, 20, 30, 1.5)", "rgba(10, 20, 30, -1)", "rgba(10, 20, 30, 50%)", "rgb(.5, .5, .5)",
  "", "not a color", "#12345", "#1234567", "xxrgb(1, 2, 3)yy",
];

export const fractionalKeys = [
  [null, null], [null, "a0"], ["a0", null], ["a0", "a1"], ["a0", "a0V"], ["a0V", "a1"], ["a1", "a2"],
  ["Zz", "a0"], ["a0", "a01"], ["zzzzzzzzzzzzzzzzzzzzzzzzzzz", null], [null, "A00000000000000000000000001"],
  ["a1", "a0"], ["a0", "a0"], ["a00", null], ["b", null], ["", null], ["a0", "a0zz"], ["a9", "aA"], ["az", "b00"],
];

export const fractionalRanges = [
  [null, null, 0], [null, null, 5], ["a0", null, 4], [null, "a0", 4], ["a0", "a1", 7], ["a0", "a0V", 3], ["a1", "a0", 2],
];

/** [label, indices (null = missing), moved positions] for syncMovedIndices / syncInvalidIndices. */
export const indexScenarios = [
  ["allValid", ["a0", "a1", "a2", "a3"], [1]],
  ["newAtEnd", ["a0", "a1", null], [2]],
  ["newAtStart", [null, "a0", "a1"], [0]],
  ["movedDown", ["a0", "a2", "a1", "a3"], [2]],
  ["movedBlock", ["a0", "a3", "a4", "a1", "a2"], [1, 2]],
  ["allMissing", [null, null, null], [0, 1, 2]],
  ["duplicate", ["a0", "a1", "a1", "a2"], [2]],
  ["invalidFormat", ["a0", "zz", "a2"], [1]],
  ["trailingZero", ["a0", "a10", "a2"], [1]],
  ["descending", ["a3", "a2", "a1", "a0"], [3]],
  ["movedWrong", ["a0", "a1", "a1V", "a1"], [0]],
];

/**
 * [case name, elements, selected ids, "left" | "right"] for moveOneLeft/moveOneRight.
 * `frame1` is a `frame` element; `fa`/`fb`/`fbDel` are its children (`frameId: "frame1"`,
 * `fbDel` also `isDeleted: true`); every other id is a plain rectangle.
 */
export const zindexCases = [
  ["right/middle", ["a", "b", "c"], ["b"], "right"],
  ["left/middle", ["a", "b", "c"], ["b"], "left"],
  ["right/top", ["a", "b", "c"], ["c"], "right"],
  ["right/twoApart", ["a", "b", "c", "d", "e"], ["a", "c"], "right"],
  ["left/block", ["a", "b", "c", "d"], ["c", "d"], "left"],
  ["right/group", ["a", "g1", "g2", "b"], ["g1", "g2"], "right"],
  ["right/withLabel", ["r", "t", "x"], ["r"], "right"],
  ["right/skipsDeleted", ["a", "del", "b"], ["a"], "right"],
  // Selecting the frame itself moves it and every child, including a deleted one, as a block.
  ["right/frameMovesWithDeletedChild", ["out1", "frame1", "fa", "fbDel", "out2"], ["frame1"], "right"],
  // Selecting a lone child restricts the target search to the frame's own children, jumping
  // over a non-child element sitting between them.
  ["right/withinFrameSkipsOutsider", ["frame1", "fa", "mid", "fb"], ["fa"], "right"],
];

/** [case name, newElement.ts function, opts]; id and seed are fixed, everything else defaults. */
export const newElementCalls = [
  ["rectangle/defaults", "newElement", { type: "rectangle", id: "r1", seed: 5, x: 10, y: 20 }],
  ["diamond/props", "newElement", {
    type: "diamond", id: "d1", seed: 6, x: 1, y: 2, width: 30, height: 40, strokeColor: "#e03131",
    backgroundColor: "#ffc9c9", fillStyle: "hachure", strokeWidth: 4, strokeStyle: "dashed", roughness: 2,
    opacity: 50, roundness: { type: 2 }, groupIds: ["g1"], locked: true,
  }],
  ["ellipse/defaults", "newElement", { type: "ellipse", id: "e1", seed: 7, x: 0, y: 0 }],
  ["line/defaults", "newLinearElement", { type: "line", id: "l1", seed: 8, x: 0, y: 0, points: [[0, 0], [10, 10]] }],
  ["arrow/defaults", "newArrowElement", { type: "arrow", id: "a1", seed: 9, x: 0, y: 0, points: [[0, 0], [10, 10]], endArrowhead: "arrow" }],
  ["arrow/noHeads", "newArrowElement", { type: "arrow", id: "a2", seed: 10, x: 0, y: 0, points: [] }],
  ["freedraw/defaults", "newFreeDrawElement", { type: "freedraw", id: "f1", seed: 11, x: 0, y: 0, points: [[0, 0]], simulatePressure: true }],
  ["freedraw/pressures", "newFreeDrawElement", {
    type: "freedraw", id: "f2", seed: 12, x: 0, y: 0, points: [[0, 0], [3, 4]], pressures: [0.5, 0.7],
    simulatePressure: false, strokeOptions: { variability: "constant", streamline: 0.5 },
  }],
  ["text/defaults", "newTextElement", { type: "text", id: "t1", seed: 13, x: 10, y: 20, text: "hello" }],
  ["text/centeredTwoLines", "newTextElement", {
    type: "text", id: "t2", seed: 14, x: 100, y: 50, text: "two\nlines", textAlign: "center",
    verticalAlign: "middle", fontSize: 28, fontFamily: 6, strokeColor: "#1971c2",
  }],
  ["text/emptyLine", "newTextElement", { type: "text", id: "t3", seed: 15, x: 0, y: 0, text: "a\n\nb", fontFamily: 8 }],
  ["text/crlfTabs", "newTextElement", {
    type: "text", id: "t4", seed: 16, x: 5, y: 5, text: "x\r\ny\tz", textAlign: "right", verticalAlign: "bottom",
  }],
  ["text/container", "newTextElement", {
    type: "text", id: "t5", seed: 17, x: 0, y: 0, text: "label", containerId: "r1", lineHeight: 1.5, fontFamily: 7,
  }],
  ["image/defaults", "newImageElement", { type: "image", id: "i1", seed: 19, x: 10, y: 20, width: 300, height: 200 }],
  ["image/attached", "newImageElement", {
    type: "image", id: "i2", seed: 20, x: 0, y: 0, width: 64, height: 48, strokeColor: "#e03131", opacity: 80,
    fileId: "abc123", status: "saved", scale: [-1, 1],
  }],
  ["text/unknownFamily", "newTextElement", { type: "text", id: "t6", seed: 18, x: 0, y: 0, text: "?", fontFamily: 42 }],
];

/** [case name, skeleton list] for convertToExcalidrawElements. Labels wrap to their container
 * and grow it (redrawTextBoundingBox) under the generator's 0.6em text metrics; the
 * labelsWrapAndGrow and arrowBetweenGrownBoxes cases exercise that. */
export const skeletonBatches = [
  ["labelsWrapAndGrow", [
    { type: "rectangle", id: "r", x: 0, y: 0, width: 100, height: 40, label: { text: "Parse the input file" } },
    { type: "diamond", id: "d", x: 200, y: 0, width: 120, height: 60, label: { text: "Is it valid?" } },
    { type: "ellipse", id: "e", x: 0, y: 200, width: 90, height: 50, label: { text: "這是很長的中文標籤" } },
  ]],
  ["arrowListedBeforeGrownBoxes", [
    { type: "arrow", x: 85, y: 15, points: [[0, 0], [210, 0]], start: { id: "a" }, end: { id: "b" } },
    { type: "rectangle", id: "a", x: 0, y: 0, width: 80, height: 30, label: { text: "first step here" } },
    { type: "rectangle", id: "b", x: 300, y: 0, width: 80, height: 30, label: { text: "second" } },
  ]],
  ["arrowBetweenGrownBoxes", [
    { type: "rectangle", id: "a", x: 0, y: 0, width: 80, height: 30, label: { text: "first step here" } },
    { type: "rectangle", id: "b", x: 300, y: 0, width: 80, height: 30, label: { text: "second" } },
    { type: "arrow", x: 85, y: 15, points: [[0, 0], [210, 0]], start: { id: "a" }, end: { id: "b" } },
  ]],
  ["shapes", [
    { type: "rectangle", x: 0, y: 0, width: 120, height: 60, strokeColor: "#1971c2", backgroundColor: "#a5d8ff" },
    { type: "diamond", x: 200, y: 0, width: 100, height: 80, fillStyle: "hachure", strokeWidth: 4, strokeStyle: "dashed" },
    { type: "ellipse", x: 0, y: 150, width: 90, height: 40, roughness: 0, opacity: 60, groupIds: ["g1"] },
  ]],
  ["labels", [
    { type: "rectangle", id: "r", x: 0, y: 0, width: 160, height: 70, label: { text: "Parser" } },
    { type: "diamond", id: "d", x: 200, y: 0, width: 200, height: 160, strokeColor: "#e03131",
      label: { text: "a\nb", textAlign: "left", verticalAlign: "top", fontSize: 16 } },
    { type: "ellipse", id: "e", x: 0, y: 200, width: 220, height: 120, label: { text: "Nunito", fontFamily: 6 } },
  ]],
  ["text", [
    { type: "text", x: 10, y: 10, text: "left" },
    { type: "text", x: 200, y: 10, text: "centered", textAlign: "center" },
    { type: "text", x: 0, y: 100, text: "code\nblock", fontFamily: 8, fontSize: 28, strokeColor: "#2f9e44" },
  ]],
  ["lines", [
    { type: "line", x: 0, y: 0, width: 100, height: 50 },
    { type: "line", x: 0, y: 100, width: 80, height: -40 },
    { type: "arrow", x: 0, y: 200 },
    { type: "arrow", x: 0, y: 300, points: [[0, 0], [60, 40], [120, 0]] },
    { type: "arrow", x: 300, y: 0, points: [[0, 0], [-100, 0]] },
    { type: "arrow", x: 300, y: 100, points: [[0, 0], [0, -80]], startArrowhead: "dot", endArrowhead: "triangle" },
  ]],
  ["bindings", [
    { type: "arrow", id: "a", x: 125, y: 30, points: [[0, 0], [70, 0]], start: { id: "r1" }, end: { id: "r2" } },
    { type: "rectangle", id: "r1", x: 0, y: 0, width: 120, height: 60, label: { text: "one" } },
    { type: "rectangle", id: "r2", x: 200, y: 0, width: 120, height: 60 },
    { type: "arrow", id: "self", x: 60, y: 65, points: [[0, 0], [0, 40], [260, 40], [260, 0]], start: { id: "r1" }, end: { id: "r2" } },
    { type: "arrow", id: "inside", x: 30, y: 30, points: [[0, 0], [200, 0]], start: { id: "r1" }, end: { id: "r1" } },
  ]],
  ["tinyTarget", [
    { type: "ellipse", id: "dot", x: 0, y: 0, width: 0.5, height: 20 },
    { type: "arrow", x: 50, y: 10, points: [[0, 0], [-45, 0]], end: { id: "dot" } },
  ]],
];

/** A bound text label's full field set, matching `element("text", ...)`'s shape. */
function label(overrides) {
  return element("text", {
    text: "hi", fontSize: 20, fontFamily: 5, textAlign: "center", verticalAlign: "middle",
    containerId: null, originalText: "hi", autoResize: true, lineHeight: 1.25,
    ...overrides,
  });
}

/**
 * [case name, elements, ids of elements to duplicate, "in-place" | "everything"] for
 * `duplicateElements`. `generate.mjs` runs each case with `randomizeSeed: true` and, for
 * "in-place", `appState.selectedGroupIds` derived from the given ids (a group counts as
 * selected when every one of its members is in `ids`) and `overrides` adding
 * `DEFAULT_GRID_SIZE / 2` to `x`/`y` (`actionDuplicateSelection`).
 */
export const duplicateCases = [
  ["singleRectangleInPlace", [element("rectangle", { id: "r" })], ["r"], "in-place"],
  ["labeledContainerDuplicatesItsLabel", [
    element("rectangle", { id: "r", boundElements: [{ id: "t", type: "text" }] }),
    label({ id: "t", containerId: "r" }),
  ], ["r"], "in-place"],
  ["arrowOnlySelectedDropsItsBindings", [
    element("rectangle", { id: "r1" }),
    element("rectangle", { id: "r2", x: 200 }),
    linear("arrow", [[0, 0], [70, 0]], {
      id: "a", x: 130, y: 30,
      startBinding: { elementId: "r1", focus: 0, gap: 5 },
      endBinding: { elementId: "r2", focus: 0, gap: 5 },
    }),
  ], ["a"], "in-place"],
  ["arrowAndBothTargetsSelectedRebindsToTheCopies", [
    element("rectangle", { id: "r1", boundElements: [{ id: "a", type: "arrow" }] }),
    element("rectangle", { id: "r2", x: 200, boundElements: [{ id: "a", type: "arrow" }] }),
    linear("arrow", [[0, 0], [70, 0]], {
      id: "a", x: 130, y: 30,
      startBinding: { elementId: "r1", focus: 0, gap: 5 },
      endBinding: { elementId: "r2", focus: 0, gap: 5 },
    }),
  ], ["r1", "r2", "a"], "in-place"],
  ["fullySelectedGroupDuplicatesTogetherUnderANewGroupId", [
    element("rectangle", { id: "g1", groupIds: ["g"] }),
    element("rectangle", { id: "g2", x: 50, groupIds: ["g"] }),
  ], ["g1", "g2"], "in-place"],
  ["everythingDuplicatesEveryElementInOrder", [
    element("rectangle", { id: "r", boundElements: [{ id: "t", type: "text" }] }),
    label({ id: "t", containerId: "r" }),
    element("ellipse", { id: "e", x: 300 }),
  ], null, "everything"],
];

/**
 * [name, call, args] for the text_wrap group. `wrapText` args are
 * [text, fontSize, fontFamily, maxWidth]; the font string is built with `getFontString`.
 * `parseTokens` takes one line.
 */
export const textWrapCases = [
  ["tokensLatin", "parseTokens", ["Hello-world, this is (a) test."]],
  ["tokensCjkPunctuation", "parseTokens", ["Hello 「世界。」🌎🗺"]],
  ["tokensKorean", "parseTokens", ["Hello(한글)"]],
  ["tokensCurrency", "parseTokens", ["Price￥100 and $5"]],
  ["tokensEmojiSequences", "parseTokens", ["👨‍👩‍👧‍👦 👍🏽 ☂️ 1️⃣ 🇨🇿🇯🇵 🏳️‍🌈"]],
  ["tokensMixed", "parseTokens", ["日本語のテキスト、English words-with-hyphens！"]],
  ["tokensDecomposed", "parseTokens", ["českyで"]],
  ["wrapLatin", "wrapText", ["The quick brown fox jumps over the lazy dog", 20, 5, 150]],
  ["wrapLongWord", "wrapText", ["Supercalifragilisticexpialidocious", 20, 5, 100]],
  ["wrapTrailingSpaces", "wrapText", ["ab      ", 20, 5, 40]],
  ["wrapHardBreaks", "wrapText", ["first line\n\nthird line is long", 16, 6, 90]],
  ["wrapCjk", "wrapText", ["這是一段很長的中文句子，需要換行。", 20, 5, 100]],
  ["wrapEmoji", "wrapText", ["hi 👨‍👩‍👧‍👦👨‍👩‍👧‍👦 there", 20, 5, 30]],
  ["wrapExactFit", "wrapText", ["abc def", 20, 5, 84]],
  ["wrapNarrowerThanAChar", "wrapText", ["abc", 20, 5, 5]],
  ["wrapNaN", "wrapText", ["a b c", 20, 5, NaN]],
  ["wrapNegative", "wrapText", ["a b c", 20, 5, -1]],
];

/** A container (id `c`) at the origin with a bound label (id `t`) reading `text`. */
function labeled(kind, [width, height], text, labelOverrides = {}, containerOverrides = {}) {
  const fontSize = labelOverrides.fontSize ?? 20;
  const lineHeight = 1.25;
  const lines = text.split("\n");
  const chars = Math.max(...lines.map((line) => [...line].length));
  return [
    element(kind, {
      id: "c", width, height, boundElements: [{ id: "t", type: "text" }], ...containerOverrides,
    }),
    element("text", {
      id: "t", width: 0.6 * fontSize * chars, height: lines.length * fontSize * lineHeight,
      text, originalText: text, fontSize, baseFontSize: fontSize, fontFamily: 5,
      textAlign: "center", verticalAlign: "middle", containerId: "c", autoResize: true,
      lineHeight, boundElements: null, ...labelOverrides,
    }),
  ];
}

const SENTENCE = "the quick brown fox jumps over the lazy dog";

/**
 * [name, elements, op] for the bound_text group. Each case builds a `Scene` from `elements`
 * (container first, then its label), then:
 * - `{redraw: textId, container: id | null}` runs redrawTextBoundingBox(text, container, scene);
 * - `{resize: {width, height, x?, y?}, handle, keepAspect, fromCenter, flipY}` mutates the
 *   container (`c`) to the given geometry, then runs handleBindTextResize.
 * Output: `[{id, x, y, width, height, text?}]` for every element, in input order.
 */
export const boundTextCases = [
  ["redrawRectangleGrows", labeled("rectangle", [120, 50], SENTENCE), { redraw: "t", container: "c" }],
  ["redrawDiamondGrows", labeled("diamond", [200, 100], SENTENCE), { redraw: "t", container: "c" }],
  ["redrawEllipseGrows", labeled("ellipse", [200, 100], SENTENCE), { redraw: "t", container: "c" }],
  ["redrawFits", labeled("rectangle", [400, 200], "hello world"), { redraw: "t", container: "c" }],
  ["redrawTop", labeled("rectangle", [400, 200], "hello", { verticalAlign: "top" }), { redraw: "t", container: "c" }],
  ["redrawBottom", labeled("rectangle", [400, 200], "hello", { verticalAlign: "bottom" }), { redraw: "t", container: "c" }],
  ["redrawLeft", labeled("rectangle", [400, 200], "hello", { textAlign: "left" }), { redraw: "t", container: "c" }],
  ["redrawRight", labeled("rectangle", [400, 200], "hello", { textAlign: "right" }), { redraw: "t", container: "c" }],
  ["redrawRotated", labeled("rectangle", [120, 50], SENTENCE, {}, { x: 30, y: 20, angle: 0.5 }), { redraw: "t", container: "c" }],
  ["redrawCjk", labeled("rectangle", [100, 40], "這是一段很長的中文標籤，需要換行"), { redraw: "t", container: "c" }],
  ["redrawHardBreak", labeled("rectangle", [300, 40], "first\nsecond line is longer"), { redraw: "t", container: "c" }],
  ["redrawLargeFont", labeled("rectangle", [100, 100], "hello world", { fontSize: 36 }), { redraw: "t", container: "c" }],
  [
    "redrawFixedWidthStandalone",
    [element("text", {
      id: "t", x: 10, y: 20, width: 90, height: 25, text: "hello big world", originalText: "hello big world",
      fontSize: 20, baseFontSize: 20, fontFamily: 5, textAlign: "left", verticalAlign: "top",
      containerId: null, autoResize: false, lineHeight: 1.25,
    })],
    { redraw: "t", container: null },
  ],
  [
    "redrawAutoResizeStandalone",
    [element("text", {
      id: "t", x: 10, y: 20, width: 10, height: 25, text: "hello big world", originalText: "hello big world",
      fontSize: 20, baseFontSize: 20, fontFamily: 5, textAlign: "left", verticalAlign: "top",
      containerId: null, autoResize: true, lineHeight: 1.25,
    })],
    { redraw: "t", container: null },
  ],
  ["resizeE", labeled("rectangle", [300, 100], SENTENCE), { resize: { width: 120, height: 100 }, handle: "e", keepAspect: false, fromCenter: false, flipY: false }],
  ["resizeW", labeled("rectangle", [300, 100], SENTENCE), { resize: { x: 180, width: 120, height: 100 }, handle: "w", keepAspect: false, fromCenter: false, flipY: false }],
  ["resizeN", labeled("rectangle", [300, 100], "hello world"), { resize: { y: 90, width: 300, height: 10 }, handle: "n", keepAspect: false, fromCenter: false, flipY: false }],
  ["resizeS", labeled("rectangle", [300, 100], "hello world"), { resize: { width: 300, height: 10 }, handle: "s", keepAspect: false, fromCenter: false, flipY: false }],
  ["resizeNKeepAspect", labeled("rectangle", [300, 100], SENTENCE), { resize: { y: 40, width: 120, height: 60 }, handle: "n", keepAspect: true, fromCenter: false, flipY: false }],
  ["resizeSKeepAspect", labeled("rectangle", [300, 100], SENTENCE), { resize: { width: 120, height: 60 }, handle: "s", keepAspect: true, fromCenter: false, flipY: false }],
  ["resizeNe", labeled("diamond", [300, 200], SENTENCE), { resize: { y: 100, width: 120, height: 100 }, handle: "ne", keepAspect: false, fromCenter: false, flipY: false }],
  ["resizeSw", labeled("ellipse", [300, 200], SENTENCE), { resize: { x: 180, width: 120, height: 100 }, handle: "sw", keepAspect: false, fromCenter: false, flipY: false }],
  ["resizeFromCenter", labeled("rectangle", [300, 100], SENTENCE), { resize: { x: 90, y: 20, width: 120, height: 60 }, handle: "se", keepAspect: false, fromCenter: true, flipY: false }],
  ["resizeFlipY", labeled("rectangle", [300, 100], SENTENCE), { resize: { width: 120, height: 40 }, handle: "se", keepAspect: false, fromCenter: false, flipY: true }],
  ["resizeRotated", labeled("rectangle", [300, 100], SENTENCE, {}, { x: 30, y: 20, angle: 0.5 }), { resize: { x: 30, y: 20, width: 120, height: 40 }, handle: "e", keepAspect: false, fromCenter: false, flipY: false }],
];


/** `p` rotated clockwise by `angle` about the center of `el`'s unrotated box. */
function aroundCenter(el, p, angle = el.angle) {
  const cx = el.x + el.width / 2;
  const cy = el.y + el.height / 2;
  const cos = Math.cos(angle);
  const sin = Math.sin(angle);
  return [(p[0] - cx) * cos - (p[1] - cy) * sin + cx, (p[0] - cx) * sin + (p[1] - cy) * cos + cy];
}

/** `labeled` with the label centered in its container and both turned by `angle`. */
function labeledAt(kind, [x, y, width, height], text, angle = 0) {
  const [container, label] = labeled(kind, [width, height], text, {}, { x, y, angle });
  label.x = x + (width - label.width) / 2;
  label.y = y + (height - label.height) / 2;
  label.angle = angle;
  return [container, label];
}

function plainText(id, x, y, text, overrides = {}) {
  return element("text", {
    id, x, y, width: 0.6 * 20 * text.length, height: 25, text, originalText: text, fontSize: 20,
    baseFontSize: 20, fontFamily: 5, textAlign: "left", verticalAlign: "top", containerId: null,
    autoResize: true, lineHeight: 1.25, ...overrides,
  });
}

const rotRect = element("rectangle", { id: "a", x: 20, y: 30, width: 100, height: 60 });
const rotEllipse = element("ellipse", { id: "a", x: 20, y: 30, width: 100, height: 60 });
const rotDiamond = element("diamond", { id: "a", x: 20, y: 30, width: 100, height: 60 });
const rotText = plainText("a", 20, 30, "hello world");
const rotFreedraw = freedraw([[0, 0], [20, 30], [50, 10], [80, 40]], { id: "a", x: 20, y: 30 });
const rotLine = linear("line", [[0, 0], [60, 40], [120, 10]], { id: "a", x: 20, y: 30 });
const resizeRect = (angle) => element("rectangle", { id: "a", x: 100, y: 100, width: 120, height: 80, angle });
const LOCAL_CORNERS = { nw: [0, 0], ne: [1, 0], sw: [0, 1], se: [1, 1], n: [0.5, 0], s: [0.5, 1], e: [1, 0.5], w: [0, 0.5] };
/** A pointer 12px past `handle` of `el` (in the element's own frame), then turned with it. */
function dragged(el, handle, [dx, dy] = [12, 9]) {
  const [fx, fy] = LOCAL_CORNERS[handle];
  const sx = handle.includes("w") ? -1 : 1;
  const sy = handle.includes("n") ? -1 : 1;
  return aroundCenter(el, [el.x + fx * el.width + sx * dx, el.y + fy * el.height + sy * dy]);
}
const pic = (overrides = {}) =>
  element("image", {
    id: "a", x: 100, y: 100, width: 200, height: 100, strokeColor: "transparent",
    status: "saved", fileId: "f1", scale: [1, 1], crop: null, ...overrides,
  });
const rotatedRect = resizeRect(0.6);
const rotatedLabeled = labeledAt("rectangle", [100, 100, 200, 100], "hello world", 0.5);
const rotatedText = plainText("a", 100, 100, "hello world", { angle: 0.5 });
const rotatedLine = linear("line", [[0, 0], [60, 40], [120, 10]], { id: "a", x: 100, y: 100, angle: 0.7 });
const pair = (angle) => [
  element("rectangle", { id: "a", x: 0, y: 0, width: 40, height: 20 }),
  element("rectangle", { id: "b", x: 60, y: 40, width: 40, height: 30, angle }),
];

/**
 * [name, elements, ids, handle, pointer, center, {shift, fromCenter, keepAspect}] for the
 * transform group. `ids` are the selected elements (bound text is never selected). `center` is
 * the common-bounds center a multi-element rotation turns around (ignored otherwise). `handle`
 * is "rotation" or a resize direction. Output: `[{id, x, y, width, height, angle, boundElements, startBinding?, endBinding?, points?,
 * fontSize?, text?}]` for every element, in input order.
 */
const NONE = { shift: false, fromCenter: false, keepAspect: false };
export const transformCases = [
  ["rotateRect", [rotRect], ["a"], "rotation", [150, 20], [70, 60], NONE],
  ["rotateEllipse", [rotEllipse], ["a"], "rotation", [150, 20], [70, 60], NONE],
  ["rotateDiamond", [rotDiamond], ["a"], "rotation", [150, 20], [70, 60], NONE],
  ["rotateText", [rotText], ["a"], "rotation", [150, 20], [70, 60], NONE],
  ["rotateFreedraw", [rotFreedraw], ["a"], "rotation", [150, 20], [60, 50], NONE],
  ["rotateLine", [rotLine], ["a"], "rotation", [150, 20], [80, 50], NONE],
  ["rotateAbove", [rotRect], ["a"], "rotation", [70, -40], [70, 60], NONE],
  ["rotateLeft", [rotRect], ["a"], "rotation", [-30, 60], [70, 60], NONE],
  ["rotateBelowLeft", [rotRect], ["a"], "rotation", [10, 140], [70, 60], NONE],
  ["rotateShiftNearMultiple", [rotRect], ["a"], "rotation", [170, 70], [70, 60], { ...NONE, shift: true }],
  ["rotateShiftBetweenMultiples", [rotRect], ["a"], "rotation", [170, 60 - 13.165], [70, 60], { ...NONE, shift: true }],
  ["rotateLabeledContainer", labeledAt("rectangle", [0, 0, 100, 50], "hello"), ["c"], "rotation", [100, 5], [50, 25], NONE],
  ["rotateLabeledEllipse", labeledAt("ellipse", [0, 0, 160, 80], "hello"), ["c"], "rotation", [20, 130], [80, 40], NONE],
  ["rotateTwo", pair(0), ["a", "b"], "rotation", [140, 30], [50, 35], NONE],
  ["rotateTwoOneRotated", pair(0.4), ["a", "b"], "rotation", [140, 30], [50, 35], NONE],
  ["rotateTwoShift", pair(0), ["a", "b"], "rotation", [140, 20], [50, 35], { ...NONE, shift: true }],
  [
    "rotateThree",
    [...pair(0.4), element("ellipse", { id: "c", x: 10, y: 90, width: 50, height: 50 })],
    ["a", "b", "c"], "rotation", [20, 150], [50, 70], NONE,
  ],
  [
    "rotateMultiWithLabeledContainer",
    [...labeledAt("rectangle", [0, 0, 100, 50], "hello"), element("rectangle", { id: "b", x: 140, y: 20, width: 40, height: 40 })],
    ["c", "b"], "rotation", [100, 5], [90, 30], NONE,
  ],
  ...["n", "s", "e", "w", "nw", "ne", "sw", "se"].map((handle) => [
    `resizeRotated_${handle}`, [rotatedRect], ["a"], handle, dragged(rotatedRect, handle), [0, 0], NONE,
  ]),
  ["resizeRotated_se_2_5", [resizeRect(2.5)], ["a"], "se", dragged(resizeRect(2.5), "se"), [0, 0], NONE],
  ["resizeRotated_se_keepAspect", [rotatedRect], ["a"], "se", dragged(rotatedRect, "se", [30, 5]), [0, 0], { ...NONE, keepAspect: true }],
  ["resizeRotated_se_fromCenter", [rotatedRect], ["a"], "se", dragged(rotatedRect, "se"), [0, 0], { ...NONE, fromCenter: true }],
  ["resizeRotated_se_flip", [rotatedRect], ["a"], "se", aroundCenter(rotatedRect, [rotatedRect.x - 30, rotatedRect.y - 20]), [0, 0], NONE],
  ["resizeRotated_text_se", [rotatedText], ["a"], "se", dragged(rotatedText, "se", [40, 20]), [0, 0], NONE],
  ["resizeRotated_labeled_e", rotatedLabeled, ["c"], "e", dragged(rotatedLabeled[0], "e", [60, 0]), [0, 0], NONE],
  ["resizeRotated_line_se", [rotatedLine], ["a"], "se", dragged(rotatedLine, "se", [30, 20]), [0, 0], NONE],
  ["resizeMultiRotated_se", pair(0.4), ["a", "b"], "se", [150, 110], [0, 0], NONE],
  ["resizeImage_se", [pic()], ["a"], "se", [340, 230], [0, 0], NONE],
  ["resizeImage_se_keepAspect", [pic()], ["a"], "se", [340, 230], [0, 0], { ...NONE, keepAspect: true }],
  ["resizeImage_se_flipBoth", [pic()], ["a"], "se", [60, 40], [0, 0], NONE],
  ["resizeImage_e_flipX", [pic()], ["a"], "e", [40, 150], [0, 0], NONE],
  ["resizeImage_flipped_e_flipBack", [pic({ scale: [-1, 1] })], ["a"], "e", [40, 150], [0, 0], NONE],
  ["resizeImage_flipped_e_stays", [pic({ scale: [-1, 1] })], ["a"], "e", [250, 150], [0, 0], NONE],
  ["resizeImage_rotated_se", [pic({ angle: 0.6 })], ["a"], "se", aroundCenter(pic({ id: "x", angle: 0.6 }), [330, 220], 0.6), [0, 0], NONE],
  ["resizeMultiImage_se", [pic(), element("rectangle", { id: "b", x: 320, y: 100, width: 40, height: 40 })], ["a", "b"], "se", [400, 260], [0, 0], NONE],
  ["resizeMultiImage_se_flipBoth", [pic(), element("rectangle", { id: "b", x: 320, y: 100, width: 40, height: 40 })], ["a", "b"], "se", [50, 60], [0, 0], NONE],
  ["resizeMultiImage_e_flipX", [pic(), element("rectangle", { id: "b", x: 320, y: 100, width: 40, height: 40 })], ["a", "b"], "e", [60, 150], [0, 0], NONE],
  ["resizeMultiImage_flipped_e_flipBack", [pic({ scale: [-1, 1] }), element("rectangle", { id: "b", x: 320, y: 100, width: 40, height: 40 })], ["a", "b"], "e", [60, 150], [0, 0], NONE],
];

/**
 * [name, elements] for the arrow_label group. `elements` is arrow `w` (bound text `t`) followed
 * by its label; each case runs handleBindTextResize on the arrow and then reads
 * `LinearElementEditor.getBoundTextElementPosition`.
 * Output: `[{id: "w", height}, {id: "t", text, width, height, x, y}]`.
 */
function arrowLabelCase(name, points, arrowOverrides = {}, text = "hello", labelOverrides = {}) {
  return [
    name,
    [
      linear("arrow", points, {
        id: "w", x: 50, y: 40, boundElements: [{ id: "t", type: "text" }], ...arrowOverrides,
      }),
      label({
        id: "t", x: 7, y: 9, width: 10, height: 25, text, originalText: text, containerId: "w",
        labelPosition: null, ...labelOverrides,
      }),
    ],
  ];
}

const CURVED = { roundness: { type: 2 } };
const LONG_LABEL = "the quick brown fox jumps over the lazy dog and keeps on running far away";

export const arrowLabelCases = [
  arrowLabelCase("twoPoints", [[0, 0], [300, 0]]),
  arrowLabelCase("twoPointsDiagonal", [[0, 0], [300, 120]]),
  arrowLabelCase("threePoints", [[0, 0], [150, 80], [300, 0]]),
  arrowLabelCase("fourPoints", [[0, 0], [100, 60], [200, -40], [320, 20]]),
  arrowLabelCase("twoPointsCurved", [[0, 0], [300, 0]], CURVED),
  arrowLabelCase("threePointsCurved", [[0, 0], [150, 80], [300, 0]], CURVED),
  arrowLabelCase("fourPointsCurved", [[0, 0], [100, 60], [200, -40], [320, 20]], CURVED),
  arrowLabelCase("fivePointsCurved", [[0, 0], [60, 50], [140, -30], [220, 40], [300, 0]], CURVED),
  arrowLabelCase("labelPositionStraight", [[0, 0], [300, 0]], {}, "hello", { labelPosition: 0.25 }),
  arrowLabelCase("labelPositionStraightMultiSegment", [[0, 0], [150, 80], [300, 0]], {}, "hello", { labelPosition: 0.7 }),
  arrowLabelCase("labelPositionCurved", [[0, 0], [150, 80], [300, 0]], CURVED, "hello", { labelPosition: 0.25 }),
  arrowLabelCase("labelPositionCurvedFourPoints", [[0, 0], [100, 60], [200, -40], [320, 20]], CURVED, "hello", { labelPosition: 0.8 }),
  arrowLabelCase("labelPositionZero", [[0, 0], [300, 0]], {}, "hello", { labelPosition: 0 }),
  arrowLabelCase("labelPositionOne", [[0, 0], [150, 80], [300, 0]], CURVED, "hello", { labelPosition: 1 }),
  arrowLabelCase("labelPositionOutOfRange", [[0, 0], [300, 0]], {}, "hello", { labelPosition: 3 }),
  arrowLabelCase("longTextWraps", [[0, 0], [600, 0]], {}, LONG_LABEL),
  arrowLabelCase("longTextOnAShortArrowUsesMinWidth", [[0, 0], [120, 0]], {}, LONG_LABEL),
  arrowLabelCase("veryShortArrow", [[0, 0], [30, 0]], {}, "hello world"),
  arrowLabelCase("tallTextGrowsTheArrow", [[0, 0], [300, 100]], {}, "a\nb\nc\nd\ne"),
  arrowLabelCase("largeFontMinWidth", [[0, 0], [100, 0]], {}, LONG_LABEL, { fontSize: 36 }),
  arrowLabelCase("rotatedTwoPoints", [[0, 0], [300, 0]], { angle: 0.5 }),
  arrowLabelCase("rotatedThreePoints", [[0, 0], [150, 80], [300, 0]], { angle: 0.5 }),
  arrowLabelCase("rotatedCurvedLabelPosition", [[0, 0], [150, 80], [300, 0]], { angle: 0.5, ...CURVED }, "hello", { labelPosition: 0.4 }),
];

/** Shapes `s` (left) and `u` (right) with arrow `w` bound from `s` to `u`. */
function boundArrowScene() {
  const binding = (elementId) => ({ elementId, focus: 0, gap: 4 });
  return [
    element("rectangle", { id: "s", x: 0, y: 0, width: 60, height: 40, boundElements: [{ id: "w", type: "arrow" }] }),
    element("rectangle", { id: "u", x: 200, y: 0, width: 60, height: 40, boundElements: [{ id: "w", type: "arrow" }] }),
    linear("arrow", [[0, 0], [136, 0]], {
      id: "w", x: 64, y: 20, startBinding: binding("s"), endBinding: binding("u"),
    }),
  ];
}

transformCases.push(
  ["rotateBoundArrowAlone", boundArrowScene(), ["w"], "rotation", [130, 90], [132, 20], NONE],
  ["rotateArrowWithItsStartTarget", boundArrowScene(), ["s", "w"], "rotation", [130, 90], [132, 20], NONE],
);

/**
 * [name, elements, call, args] for the intersect group. `call` is one of:
 *  - "intersectElementWithLineSegment": args `[id, [[x1, y1], [x2, y2]], offset]`; output is the
 *    array of crossing points, in the order JS returns them.
 *  - "getHoveredElementForBinding": args `[[x, y], tolerance]` over all `elements`; output is the
 *    id of the element to bind to, or null.
 *  - "hitElementItself": args `[id, [x, y], threshold, overrideShouldTestInside]`; output is a boolean.
 */
const ROUND = { type: 3 };
const box = (type, overrides = {}) => element(type, { id: "a", x: 100, y: 100, width: 200, height: 100, ...overrides });
const outlineShapes = [
  ["rect", box("rectangle")],
  ["roundRect", box("rectangle", { roundness: ROUND })],
  ["rotatedRect", box("rectangle", { angle: 0.6 })],
  ["rotatedRoundRect", box("rectangle", { angle: 0.6, roundness: ROUND })],
  ["diamond", box("diamond")],
  ["roundDiamond", box("diamond", { roundness: ROUND })],
  ["rotatedRoundDiamond", box("diamond", { angle: 0.6, roundness: ROUND })],
  ["ellipse", box("ellipse")],
  ["rotatedEllipse", box("ellipse", { angle: 0.6 })],
  ["text", plainText("a", 100, 100, "hello world", { width: 200, height: 100 })],
  ["image", pic({ width: 200, height: 100 })],
];
const crossings = [
  ["horizontal", [[40, 130], [360, 130]]],
  ["diagonal", [[60, 70], [340, 200]]],
  ["vertical", [[220, 60], [220, 240]]],
];

export const intersectCases = [];
for (const [label, shape] of outlineShapes) {
  for (const offset of [0, 6]) {
    for (const [direction, segment] of crossings) {
      intersectCases.push([`${label}/${direction}/offset${offset}`, [shape], "intersectElementWithLineSegment", ["a", segment, offset]]);
    }
  }
  intersectCases.push([`${label}/ends inside`, [shape], "intersectElementWithLineSegment", ["a", [[150, 140], [250, 160]], 0]]);
  intersectCases.push([`${label}/far away`, [shape], "intersectElementWithLineSegment", ["a", [[0, 0], [50, 40]], 6]]);
  intersectCases.push([`${label}/beside the outline`, [shape], "intersectElementWithLineSegment", ["a", [[100, 100], [140, 100]], 0]]);
}
// A segment that cuts the corner of a rounded shape crosses the curve, not a side.
for (const offset of [0, 6]) {
  intersectCases.push([`roundRect/corner/offset${offset}`, [box("rectangle", { roundness: ROUND })], "intersectElementWithLineSegment", ["a", [[70, 70], [130, 130]], offset]]);
  intersectCases.push([`rotatedRoundRect/corner/offset${offset}`, [box("rectangle", { angle: 0.6, roundness: ROUND })], "intersectElementWithLineSegment", ["a", [[70, 150], [160, 150]], offset]]);
  intersectCases.push([`roundDiamond/corner/offset${offset}`, [box("diamond", { roundness: ROUND })], "intersectElementWithLineSegment", ["a", [[290, 150], [340, 150]], offset]]);
}
// Open and closed lines and freedraw strokes go through the collision shape.
const wave = linear("line", [[0, 0], [60, 40], [120, 10], [200, 60]], { id: "a", x: 100, y: 100, roundness: ROUND });
const waveSharp = linear("line", [[0, 0], [60, 40], [120, 10], [200, 60]], { id: "a", x: 100, y: 100 });
for (const [label, shape] of [["roundLine", wave], ["sharpLine", waveSharp], ["rotatedLine", { ...waveSharp, angle: 0.5 }]]) {
  intersectCases.push([`${label}/horizontal`, [shape], "intersectElementWithLineSegment", ["a", [[80, 120], [320, 120]], 0]]);
  intersectCases.push([`${label}/vertical`, [shape], "intersectElementWithLineSegment", ["a", [[150, 60], [150, 220]], 0]]);
}

const filled = "#ffc9c9";
const pointInside = [200, 150];
export const hoverCases = [
  ["nestedTransparentPicksSmaller", [box("rectangle", { id: "big", x: 0, y: 0, width: 400, height: 300 }), box("rectangle", { id: "small", x: 100, y: 100, width: 100, height: 80 })], "getHoveredElementForBinding", [[150, 140], 15]],
  ["nestedTransparentSmallerBehind", [box("rectangle", { id: "small", x: 100, y: 100, width: 100, height: 80 }), box("rectangle", { id: "big", x: 0, y: 0, width: 400, height: 300 })], "getHoveredElementForBinding", [[150, 140], 15]],
  ["filledFrontStopsTheSearch", [box("rectangle", { id: "big", x: 0, y: 0, width: 400, height: 300 }), box("rectangle", { id: "small", x: 100, y: 100, width: 100, height: 80, backgroundColor: filled })], "getHoveredElementForBinding", [[150, 140], 15]],
  ["filledBigFrontTransparentSmallBehind", [box("rectangle", { id: "small", x: 100, y: 100, width: 100, height: 80 }), box("rectangle", { id: "big", x: 0, y: 0, width: 400, height: 300, backgroundColor: filled })], "getHoveredElementForBinding", [[150, 140], 15]],
  ["filledBehindTransparentFront", [box("rectangle", { id: "big", x: 0, y: 0, width: 400, height: 300, backgroundColor: filled }), box("rectangle", { id: "small", x: 100, y: 100, width: 100, height: 80 })], "getHoveredElementForBinding", [[150, 140], 15]],
  ["equalSizesPickBehind", [box("rectangle", { id: "back" }), box("rectangle", { id: "front" })], "getHoveredElementForBinding", [pointInside, 15]],
  ["outside10Tolerance15", [box("rectangle")], "getHoveredElementForBinding", [[200, 90], 15]],
  ["outside10Tolerance5", [box("rectangle")], "getHoveredElementForBinding", [[200, 90], 5]],
  ["insideTolerance5", [box("rectangle")], "getHoveredElementForBinding", [pointInside, 5]],
  ["farAway", [box("rectangle")], "getHoveredElementForBinding", [[600, 600], 15]],
  ["lockedIsSkipped", [box("rectangle", { locked: true })], "getHoveredElementForBinding", [pointInside, 15]],
  ["lockedFrontFallsThrough", [box("rectangle", { id: "back" }), box("rectangle", { id: "front", locked: true, backgroundColor: filled })], "getHoveredElementForBinding", [pointInside, 15]],
  ["boundTextIsSkipped", [plainText("t", 100, 100, "hello world", { containerId: "c", width: 200, height: 100 })], "getHoveredElementForBinding", [pointInside, 15]],
  ["containerWithLabel", labeledAt("rectangle", [100, 100, 200, 100], "hello"), "getHoveredElementForBinding", [pointInside, 15]],
  ["freeText", [plainText("t", 100, 100, "hello world", { width: 200, height: 100 })], "getHoveredElementForBinding", [[150, 90], 15]],
  ["lineIsNotATarget", [linear("line", [[0, 0], [200, 0]], { id: "l", x: 100, y: 150 })], "getHoveredElementForBinding", [[200, 150], 15]],
  ["ellipseInside", [box("ellipse")], "getHoveredElementForBinding", [pointInside, 15]],
  ["ellipseCornerOfBounds", [box("ellipse")], "getHoveredElementForBinding", [[105, 105], 5]],
  ["diamondInside", [box("diamond")], "getHoveredElementForBinding", [pointInside, 15]],
  ["diamondNearEdge", [box("diamond")], "getHoveredElementForBinding", [[160, 112], 15]],
  ["imageNearEdge", [pic({ width: 200, height: 100 })], "getHoveredElementForBinding", [[310, 150], 15]],
  ["rotatedRectNearEdge", [box("rectangle", { angle: 0.6 })], "getHoveredElementForBinding", [[200, 90], 15]],
  ["rotatedRoundRectInside", [box("rectangle", { angle: 0.6, roundness: ROUND })], "getHoveredElementForBinding", [[230, 140], 15]],
];

const hitShape = (type, overrides = {}) => box(type, overrides);
export const hitCases = [
  ["transparentRectInside/override", [hitShape("rectangle")], "hitElementItself", ["a", [200, 150], 8, true]],
  ["transparentRectInside/plain", [hitShape("rectangle")], "hitElementItself", ["a", [201, 151], 8, false]],
  ["transparentRectOutline/override", [hitShape("rectangle")], "hitElementItself", ["a", [200, 103], 8, true]],
  ["transparentRectOutline/plain", [hitShape("rectangle")], "hitElementItself", ["a", [201, 103], 8, false]],
  ["filledRectInside/plain", [hitShape("rectangle", { backgroundColor: filled })], "hitElementItself", ["a", [202, 151], 8, false]],
  ["filledRectInside/override", [hitShape("rectangle", { backgroundColor: filled })], "hitElementItself", ["a", [203, 151], 8, true]],
  ["transparentEllipseInside/override", [hitShape("ellipse")], "hitElementItself", ["a", [204, 150], 8, true]],
  ["transparentEllipseInside/plain", [hitShape("ellipse")], "hitElementItself", ["a", [205, 150], 8, false]],
  ["transparentDiamondInside/override", [hitShape("diamond")], "hitElementItself", ["a", [206, 150], 8, true]],
  ["transparentDiamondInside/plain", [hitShape("diamond")], "hitElementItself", ["a", [207, 150], 8, false]],
  ["rotatedRectInside/override", [hitShape("rectangle", { angle: 0.6 })], "hitElementItself", ["a", [208, 150], 8, true]],
  ["rotatedRectInside/plain", [hitShape("rectangle", { angle: 0.6 })], "hitElementItself", ["a", [209, 150], 8, false]],
  ["outside/override", [hitShape("rectangle")], "hitElementItself", ["a", [200, 60], 8, true]],
  ["imageInside/plain", [pic({ width: 200, height: 100 })], "hitElementItself", ["a", [210, 150], 8, false]],
  ["textInside/plain", [plainText("a", 100, 100, "hello world", { width: 200, height: 100 })], "hitElementItself", ["a", [211, 150], 8, false]],
];

intersectCases.push(...hoverCases, ...hitCases);

/**
 * [name, elements, ids, op] for the bound_follow group: bound arrows after a gesture on the
 * selected `ids`. `op` is `{drag: [dx, dy]}` (`dragSelectedElements`) or
 * `{transform: [handle, pointer, center, flags]}` (`transformElements`). Every binding is in the
 * current `{elementId, mode, fixedPoint}` format. Arrows default to an arrowhead at the end
 * only; `heads: "none"` and `heads: "start"` exercise the other `resolvedTarget` branches.
 * Arrow labels start at their computed position (the generator settles them).
 */
const arrowRef = (id) => ({ id, type: "arrow" });
const shapeAt = (type, id, [x, y, width, height], arrows = [], overrides = {}) =>
  element(type, {
    id, x, y, width, height, boundElements: arrows.map(arrowRef), ...overrides,
  });
const bindTo = (elementId, fixedPoint, mode = "orbit") => ({ elementId, mode, fixedPoint });
const HEADS = {
  end: { startArrowhead: null, endArrowhead: "arrow" },
  none: { startArrowhead: null, endArrowhead: null },
  start: { startArrowhead: "arrow", endArrowhead: null },
  both: { startArrowhead: "arrow", endArrowhead: "arrow" },
};
function boundArrow(id, [x, y], points, start, end, { heads = "end", ...overrides } = {}) {
  return linear("arrow", points, {
    id, x, y, startBinding: start, endBinding: end, ...HEADS[heads], ...overrides,
  });
}
function arrowLabel(arrowId, text = "hello") {
  return label({
    id: `${arrowId}-label`, x: 0, y: 0, width: 10, height: 25, text, originalText: text,
    containerId: arrowId, labelPosition: null,
  });
}
const withLabel = (arrow) => [
  { ...arrow, boundElements: [{ id: `${arrow.id}-label`, type: "text" }] },
  arrowLabel(arrow.id),
];

// `r1` (0..100) and `r2` (300..400) are 100x100 rectangles with stroke 2, so an arrow end
// one binding gap (6) outside an edge is at x 106 (r1 right) or 294 (r2 left); the fixed
// points are those positions as ratios of the shape.
const R1_RIGHT = [1.06, 0.5001];
const R2_LEFT = [-0.06, 0.5001];
const r1At = (arrows, over) => shapeAt("rectangle", "r1", [0, 0, 100, 100], arrows, over);
const r2At = (arrows, over) => shapeAt("rectangle", "r2", [300, 0, 100, 100], arrows, over);
const oneEnd = (heads, r1Over) => [
  r1At(["w"], r1Over),
  boundArrow("w", [106, 50], [[0, 0], [188, 0]], bindTo("r1", R1_RIGHT), null, { heads }),
];
const twoEnds = (heads, over = {}) => [
  r1At(["w"]),
  r2At(["w"]),
  boundArrow("w", [106, 50], [[0, 0], [188, 0]], bindTo("r1", R1_RIGHT), bindTo("r2", R2_LEFT), { heads, ...over }),
];
const drag = (dx, dy) => ({ drag: [dx, dy] });
const transform = (handle, pointer, center = [0, 0], flags = NONE) => ({ transform: [handle, pointer, center, flags] });

export const boundFollowCases = [
  ["dragStartTarget", oneEnd("end"), ["r1"], drag(40, 60)],
  ["dragStartTargetFar", oneEnd("end"), ["r1"], drag(-200, 150)],
  ["dragStartTargetNoArrowheads", oneEnd("none"), ["r1"], drag(40, 60)],
  ["dragStartTargetStartArrowhead", oneEnd("start"), ["r1"], drag(40, 60)],
  ["dragStartTargetBothArrowheads", oneEnd("both"), ["r1"], drag(40, 60)],
  ["dragTargetWithBothEndsBound_first", twoEnds("end"), ["r1"], drag(40, 60)],
  ["dragTargetWithBothEndsBound_second", twoEnds("end"), ["r2"], drag(0, 80)],
  ["dragTargetWithBothEndsBound_noArrowheads", twoEnds("none"), ["r2"], drag(-30, 80)],
  ["dragBothTargets", twoEnds("end"), ["r1", "r2"], drag(25, 70)],
  ["dragTargetAndArrow", oneEnd("end"), ["r1", "w"], drag(40, 60)],
  ["dragTargetAndArrowOtherEndStays", twoEnds("end"), ["r1", "w"], drag(40, 60)],
  [
    "dragSameTargetBothEnds_twoPoints",
    [
      r1At(["w"]),
      boundArrow("w", [50, 50], [[0, 0], [0, -56]], bindTo("r1", [0.5001, 0.5001]), bindTo("r1", [0.5001, 0.0])),
    ],
    ["r1"], drag(200, 30),
  ],
  [
    "dragSameTargetBothEnds_fourPoints",
    [
      r1At(["w"]),
      boundArrow("w", [106, 50], [[0, 0], [40, 0], [40, -90], [-56, -56]], bindTo("r1", R1_RIGHT), bindTo("r1", [0.5001, -0.06])),
    ],
    ["r1"], drag(200, 30),
  ],
  [
    "dragThreePointStart",
    [
      r1At(["w"]),
      boundArrow("w", [106, 50], [[0, 0], [100, 60], [188, 0]], bindTo("r1", R1_RIGHT), null),
    ],
    ["r1"], drag(0, -40),
  ],
  [
    "dragThreePointEnd",
    [
      r2At(["w"]),
      boundArrow("w", [106, 50], [[0, 0], [100, 60], [188, 0]], null, bindTo("r2", R2_LEFT)),
    ],
    ["r2"], drag(30, 50),
  ],
  [
    "dragThreePointBothEnds",
    [
      r1At(["w"]),
      r2At(["w"]),
      boundArrow("w", [106, 50], [[0, 0], [100, 60], [188, 0]], bindTo("r1", R1_RIGHT), bindTo("r2", R2_LEFT)),
    ],
    ["r2"], drag(30, 50),
  ],
  [
    "dragInsideStart",
    [
      r1At(["w"]),
      boundArrow("w", [30, 40], [[0, 0], [264, 10]], bindTo("r1", [0.3, 0.4], "inside"), null),
    ],
    ["r1"], drag(50, 50),
  ],
  [
    "dragInsideEnd",
    [
      r2At(["w"]),
      boundArrow("w", [106, 50], [[0, 0], [224, 40]], null, bindTo("r2", [0.2, 0.9], "inside")),
    ],
    ["r2"], drag(-60, 20),
  ],
  [
    "dragInsideOneEndOrbitOther",
    [
      r1At(["w"]),
      r2At(["w"]),
      boundArrow("w", [50, 50], [[0, 0], [244, 0]], bindTo("r1", [0.5001, 0.5001], "inside"), bindTo("r2", R2_LEFT)),
    ],
    ["r1"], drag(20, 90),
  ],
  ["dragLabeledArrowTarget", [...twoEnds("end").slice(0, 2), ...withLabel(twoEnds("end")[2])], ["r2"], drag(0, 80)],
  ["dragLabeledArrowTargetFar", [...twoEnds("end").slice(0, 2), ...withLabel(twoEnds("end")[2])], ["r1"], drag(-150, 200)],
  [
    "dragLabeledOneEnd",
    (() => {
      const [r1, w] = oneEnd("end");
      return [r1, ...withLabel(w)];
    })(),
    ["r1"], drag(60, -30),
  ],
  [
    "dragDiamondTarget",
    [
      shapeAt("diamond", "r1", [0, 0, 100, 100], ["w"]),
      boundArrow("w", [106, 50], [[0, 0], [188, 0]], bindTo("r1", [1.06, 0.5001]), null),
    ],
    ["r1"], drag(40, 60),
  ],
  [
    "dragEllipseTarget",
    [
      shapeAt("ellipse", "r1", [0, 0, 100, 100], ["w"]),
      boundArrow("w", [106, 50], [[0, 0], [188, 0]], bindTo("r1", [1.06, 0.5001]), null),
    ],
    ["r1"], drag(40, 60),
  ],
  [
    "dragRotatedRectangleTarget",
    [
      shapeAt("rectangle", "r1", [0, 0, 100, 100], ["w"], { angle: 0.6 }),
      boundArrow("w", [106, 50], [[0, 0], [188, 0]], bindTo("r1", [1.06, 0.5001]), null),
    ],
    ["r1"], drag(40, 60),
  ],
  [
    "dragRotatedWideRectangleBothEnds",
    [
      shapeAt("rectangle", "r1", [0, 0, 160, 60], ["w"], { angle: 0.6 }),
      r2At(["w"]),
      boundArrow("w", [166, 30], [[0, 0], [128, 20]], bindTo("r1", [1.04, 0.5001]), bindTo("r2", R2_LEFT)),
    ],
    ["r1"], drag(30, 90),
  ],
  [
    "dragTextTarget",
    [
      plainText("r1", 0, 0, "hello", { width: 100, height: 25, boundElements: [arrowRef("w")] }),
      boundArrow("w", [106, 12.5], [[0, 0], [188, 0]], bindTo("r1", [1.06, 0.5001]), null),
    ],
    ["r1"], drag(40, 60),
  ],
  [
    "dragLockedTarget",
    oneEnd("end", { locked: true }),
    ["r1"], drag(40, 60),
  ],
  [
    "dragStaleBoundElementsEntry",
    [
      r1At(["w", "gone"]),
      boundArrow("w", [106, 50], [[0, 0], [188, 0]], bindTo("r1", R1_RIGHT), null),
    ],
    ["r1"], drag(40, 60),
  ],
  [
    "dragZeroSizeTarget",
    [
      shapeAt("rectangle", "r1", [50, 50, 0, 0], ["w"]),
      boundArrow("w", [56, 50], [[0, 0], [238, 0]], bindTo("r1", [0.5001, 0.5001]), null),
    ],
    ["r1"], drag(40, 60),
  ],
  [
    "dragOverlappingTargets_similar",
    [
      shapeAt("rectangle", "r1", [0, 0, 100, 100], ["w"]),
      shapeAt("rectangle", "r2", [60, 20, 100, 100], ["w"]),
      boundArrow("w", [50, 50], [[0, 0], [60, 20]], bindTo("r1", [0.5001, 0.5001]), bindTo("r2", [0.5001, 0.5001])),
    ],
    ["r2"], drag(10, 0),
  ],
  [
    "dragOverlappingTargets_inner",
    [
      shapeAt("rectangle", "r1", [0, 0, 400, 400], ["w"]),
      shapeAt("rectangle", "r2", [150, 150, 100, 100], ["w"]),
      boundArrow("w", [200, 200], [[0, 0], [206, 0]], bindTo("r2", [0.5001, 0.5001]), bindTo("r1", [1.015, 0.5001])),
    ],
    ["r1"], drag(30, 20),
  ],
  [
    "dragOverlappingTargets_innerMoved",
    [
      shapeAt("rectangle", "r1", [0, 0, 400, 400], ["w"]),
      shapeAt("rectangle", "r2", [150, 150, 100, 100], ["w"]),
      boundArrow("w", [200, 200], [[0, 0], [206, 0]], bindTo("r2", [0.5001, 0.5001]), bindTo("r1", [1.015, 0.5001])),
    ],
    ["r2"], drag(60, -40),
  ],
  [
    "dragTargetsOverlapAfterMove",
    twoEnds("end"),
    ["r2"], drag(-250, 20),
  ],
  [
    "dragMakesArrowTooShort",
    [
      r1At(["w"]),
      shapeAt("rectangle", "r2", [130, 0, 100, 100], ["w"]),
      boundArrow("w", [106, 50], [[0, 0], [18, 0]], bindTo("r1", R1_RIGHT), bindTo("r2", R2_LEFT)),
    ],
    ["r2"], drag(-20, 0),
  ],
  [
    "dragMakesArrowTooShort_noArrowheads",
    [
      r1At(["w"]),
      shapeAt("rectangle", "r2", [130, 0, 100, 100], ["w"]),
      boundArrow("w", [106, 50], [[0, 0], [18, 0]], bindTo("r1", R1_RIGHT), bindTo("r2", R2_LEFT), { heads: "none" }),
    ],
    ["r2"], drag(-20, 0),
  ],
  [
    "dragMakesFreeEndArrowTooShort",
    oneEnd("end"),
    ["r1"], drag(170, 0),
  ],
  [
    "resizeBoundRectangle_se",
    oneEnd("end"),
    ["r1"], transform("se", [160, 140]),
  ],
  [
    "resizeBoundRectangle_nw",
    oneEnd("end"),
    ["r1"], transform("nw", [-30, -20]),
  ],
  [
    "resizeBoundRectangle_e_keepAspect",
    twoEnds("end"),
    ["r1"], transform("e", [150, 50], [0, 0], { ...NONE, keepAspect: true }),
  ],
  [
    "resizeBoundRectangle_se_fromCenter",
    twoEnds("end"),
    ["r2"], transform("se", [430, 120], [0, 0], { ...NONE, fromCenter: true }),
  ],
  [
    "resizeBoundRectangle_flip",
    oneEnd("end"),
    ["r1"], transform("se", [-40, -30]),
  ],
  [
    "resizeBoundRotatedRectangle_se",
    [
      shapeAt("rectangle", "r1", [0, 0, 100, 100], ["w"], { angle: 0.6 }),
      boundArrow("w", [106, 50], [[0, 0], [188, 0]], bindTo("r1", [1.06, 0.5001]), null),
    ],
    ["r1"], transform("se", aroundCenter(element("rectangle", { x: 0, y: 0, width: 100, height: 100, angle: 0.6 }), [130, 125], 0.6)),
  ],
  [
    "resizeBoundEllipse_e",
    [
      shapeAt("ellipse", "r1", [0, 0, 100, 100], ["w"]),
      boundArrow("w", [106, 50], [[0, 0], [188, 0]], bindTo("r1", [1.06, 0.5001]), null),
    ],
    ["r1"], transform("e", [160, 50]),
  ],
  [
    "resizeBoundLabeledArrowTarget",
    [...twoEnds("end").slice(0, 2), ...withLabel(twoEnds("end")[2])],
    ["r1"], transform("se", [160, 140]),
  ],
  [
    "resizeBoundText_se",
    [
      plainText("r1", 0, 0, "hello", { width: 100, height: 25, boundElements: [arrowRef("w")] }),
      boundArrow("w", [106, 12.5], [[0, 0], [188, 0]], bindTo("r1", [1.06, 0.5001]), null),
    ],
    ["r1"], transform("se", [140, 40]),
  ],
  [
    "resizeBoundText_e",
    [
      plainText("r1", 0, 0, "hello", { width: 100, height: 25, boundElements: [arrowRef("w")] }),
      boundArrow("w", [106, 12.5], [[0, 0], [188, 0]], bindTo("r1", [1.06, 0.5001]), null),
    ],
    ["r1"], transform("e", [150, 12]),
  ],
  [
    "resizeTwoShapes_e",
    [
      r1At(["w"]),
      r2At([]),
      boundArrow("w", [106, 50], [[0, 0], [188, 0]], bindTo("r1", R1_RIGHT), null),
    ],
    ["r1", "r2"], transform("e", [600, 50]),
  ],
  [
    "resizeTwoBoundShapes_se",
    twoEnds("end"),
    ["r1", "r2"], transform("se", [500, 150]),
  ],
  [
    "resizeShapesAndArrow_e",
    twoEnds("end"),
    ["r1", "r2", "w"], transform("e", [500, 50]),
  ],
  [
    "rotateBoundRectangle",
    oneEnd("end"),
    ["r1"], transform("rotation", [150, 20], [50, 50]),
  ],
  [
    "rotateBoundRectangleBothEnds",
    twoEnds("end"),
    ["r2"], transform("rotation", [450, 20], [350, 50]),
  ],
  [
    "rotateBoundDiamond",
    [
      shapeAt("diamond", "r1", [0, 0, 100, 100], ["w"]),
      boundArrow("w", [106, 50], [[0, 0], [188, 0]], bindTo("r1", [1.06, 0.5001]), null),
    ],
    ["r1"], transform("rotation", [150, 20], [50, 50]),
  ],
  [
    "rotateBoundInside",
    [
      r1At(["w"]),
      boundArrow("w", [30, 40], [[0, 0], [264, 10]], bindTo("r1", [0.3, 0.4], "inside"), null),
    ],
    ["r1"], transform("rotation", [150, 20], [50, 50]),
  ],
  [
    "rotateBoundLabeledArrowTarget",
    [...twoEnds("end").slice(0, 2), ...withLabel(twoEnds("end")[2])],
    ["r1"], transform("rotation", [150, 20], [50, 50]),
  ],
  [
    "rotateTwoBoundShapes",
    twoEnds("end"),
    ["r1", "r2"], transform("rotation", [200, 160], [200, 50]),
  ],
  [
    "rotateShapeAndArrow",
    twoEnds("end"),
    ["r1", "w"], transform("rotation", [200, 160], [200, 50]),
  ],
];
