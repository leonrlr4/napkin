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
];
