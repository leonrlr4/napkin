// Inputs for every baseline group. Each case is { name, call, args }; `args` is the exact
// positional argument list, options object last. Names must be unique per group.

const SEEDS = [1, 12345, 2147483647];
const ROUGHNESS = [0, 1, 2];
const FILL_SEEDS = [1, 2147483647];

function cross(prefix, call, argsFor, variants) {
  const cases = [];
  for (const [label, extra] of variants) {
    cases.push({ name: `${prefix}/${label}`, call, args: argsFor(extra) });
  }
  return cases;
}

/** seed × roughness variants merged into `base` options. */
function seedRoughness(base = {}, seeds = SEEDS) {
  const variants = [];
  for (const seed of seeds) {
    for (const roughness of ROUGHNESS) {
      variants.push([`s${seed}/r${roughness}`, { ...base, seed, roughness }]);
    }
  }
  return variants;
}

// --- geometry --------------------------------------------------------------------

const LINES = {
  mid: [10, 20, 250, 180],
  short: [0, 0, 3, 4],
  long: [0, 0, 700, 0],
  vertical: [5, 5, 5, 305],
};
const POLYLINES = {
  empty: [],
  single: [[3, 4]],
  two: [[0, 0], [120, 40]],
  zigzag: [[0, 0], [60, 80], [120, 10], [180, 90], [240, 0]],
};
const POLYGONS = {
  triangle: [[10, 10], [110, 30], [40, 90]],
  concave: [[0, 0], [100, 0], [100, 80], [50, 30], [0, 80]],
};
const RECTS = {
  normal: [5, 10, 110, 70],
  tiny: [0, 0, 8, 6],
  negative: [100, 100, -80, -40],
};
const ELLIPSES = {
  normal: [60, 40, 110, 70],
  tiny: [10, 10, 6, 4],
};
const ARCS = {
  half: [60, 60, 100, 80, 0, Math.PI, false],
  closed: [60, 60, 100, 80, -Math.PI / 2, Math.PI, true],
  overfull: [60, 60, 100, 80, 0, 7, true],
};
const CURVES = {
  two: [[0, 0], [100, 50]],
  three: [[0, 0], [50, 80], [100, 0]],
  wave: [[0, 0], [30, 60], [70, -20], [110, 50], [140, 0], [160, 40]],
};
// Path strings mirror what Excalidraw builds (shape.ts) plus every SVG command form
// path-data-parser handles.
const PATHS = {
  roundedRect: "M 15 0 L 95 0 Q 110 0, 110 15 L 110 55 Q 110 70, 95 70 L 15 70 Q 0 70, 0 55 L 0 15 Q 0 0, 15 0",
  diamondRounded: `M 56 7 L 103 28
            C 110 35, 110 35, 103 42
            L 63 63
            C 56 70, 56 70, 49 63
            L 7 42
            C 0 35, 0 35, 7 28
            L 49 7
            C 56 0, 56 0, 56 7`,
  relative: "m 10 10 l 40 0 h 20 v 30 c 0 10 -10 20 -20 20 s -20 -10 -20 -20 q 0 -15 10 -20 t 10 -10 z",
  absoluteCurves: "M 0 0 H 50 V 40 C 60 50 70 50 80 40 S 100 30 110 40 Q 120 60 100 70 T 60 80 Z",
  arcs: "M 10 50 A 40 30 0 0 1 90 50 A 40 30 0 1 0 10 50 a 20 20 45 1 1 30 30 A 0 10 0 0 0 20 20",
  implicitRepeats: "M 0 0 10 10 20 0 30 10 L 40 0 50 10",
  numbers: "M1e1,2E1L.5-3.5,+7 8.25e-1",
  noMove: "L 30 30 L 60 0",
  subpaths: "M 0 0 L 40 0 L 40 40 Z M 60 0 L 100 0 L 80 40 Z",
  elbow: "M 0 0 L 34 0 Q 50 0, 50 16 L 50 84 Q 50 100, 66 100 L 150 100",
};
const BAD_PATHS = {
  invalidChar: "M 0 0 X 10 10",
  paramNotNumber: "M 0 L 10 10",
  endedShort: "M 0",
};

// --- groups -----------------------------------------------------------------------

const random = [1, 2, 12345, 2147483647, 2147483648, -7, 1.5].map((seed) => ({
  name: `seed${seed}`,
  call: "Random",
  args: [seed, 12],
}));

const pathData = [];
for (const call of ["parsePath", "absolutize", "normalize"]) {
  for (const [label, d] of Object.entries({ ...PATHS, ...BAD_PATHS })) {
    pathData.push({ name: `${call}/${label}`, call, args: [d] });
  }
}

const BEZIER = [[0, 0], [30, 80], [90, -40], [120, 40], [150, 90], [200, 10], [240, 60]];
const NOISY = Array.from({ length: 30 }, (_, i) => [i * 7, Math.round(40 * Math.sin(i / 3)) + (i % 4)]);
const pointsOnCurve = [
  ...[0.15, 1, 10].flatMap((tolerance) =>
    [undefined, 0.5, 3].map((distance) => ({
      name: `pointsOnBezierCurves/t${tolerance}/d${distance}`,
      call: "pointsOnBezierCurves",
      args: distance === undefined ? [BEZIER, tolerance] : [BEZIER, tolerance, distance],
    })),
  ),
  ...[0.75, 2, 10].map((distance) => ({ name: `simplify/d${distance}`, call: "simplify", args: [NOISY, distance] })),
  ...Object.entries({ ...CURVES, single: [[1, 1]] }).flatMap(([label, points]) =>
    [0, 0.5].map((tightness) => ({
      name: `curveToBezier/${label}/k${tightness}`,
      call: "curveToBezier",
      args: [points, tightness],
    })),
  ),
];

const pointsOnPath = Object.entries(PATHS).flatMap(([label, d]) =>
  [
    [1, undefined],
    [1, 1.5],
    [0.15, 0.5],
  ].map(([tolerance, distance]) => ({
    name: `pointsOnPath/${label}/t${tolerance}/d${distance}`,
    call: "pointsOnPath",
    args: distance === undefined ? [d, tolerance] : [d, tolerance, distance],
  })),
);

const hachureFill = [];
for (const [label, polygons] of Object.entries({
  triangle: [POLYGONS.triangle],
  concave: [POLYGONS.concave],
  twoPolygons: [POLYGONS.triangle, [[150, 0], [220, 0], [220, 60], [150, 60]]],
  closedAlready: [[[0, 0], [80, 0], [80, 50], [0, 0]]],
  degenerate: [[[0, 0], [50, 0], [100, 0]]],
})) {
  for (const [gap, angle, step] of [
    [4, 49, 1],
    [8, 0, 1],
    [8, 131, 8],
    [2.5, -41, 1],
    [0.01, 90, 1],
  ]) {
    hachureFill.push({
      name: `${label}/g${gap}/a${angle}/o${step}`,
      call: "hachureLines",
      args: [polygons, gap, angle, step],
    });
  }
}

const OPTION_SWEEP = [
  ["bowing0", { bowing: 0 }],
  ["bowing3", { bowing: 3 }],
  ["maxOffset0", { maxRandomnessOffset: 0 }],
  ["maxOffset5", { maxRandomnessOffset: 5 }],
  ["roughness0.5", { roughness: 0.5 }],
  ["singleStroke", { disableMultiStroke: true }],
  ["preserveVertices", { preserveVertices: true }],
  ["strokeWidth4", { strokeWidth: 4 }],
];
const ELLIPTIC_SWEEP = [
  ["curveFitting1", { curveFitting: 1 }],
  ["curveFitting0.5", { curveFitting: 0.5 }],
  ["curveStepCount4", { curveStepCount: 4 }],
  ["curveStepCount20", { curveStepCount: 20 }],
  ["singleStroke", { disableMultiStroke: true }],
  ["roughness0.5", { roughness: 0.5 }],
];

function withSweep(variants, sweep, seed = 12345) {
  return [...variants, ...sweep.map(([label, o]) => [`sweep/${label}`, { seed, roughness: 1, ...o }])];
}

const outlineLinear = [
  ...Object.entries(LINES).flatMap(([label, a]) =>
    cross(`line/${label}`, "line", (o) => [...a, o], withSweep(seedRoughness(), OPTION_SWEEP)),
  ),
  ...Object.entries(POLYLINES).flatMap(([label, p]) =>
    cross(`linearPath/${label}`, "linearPath", (o) => [p, o], seedRoughness()),
  ),
  ...Object.entries(POLYGONS).flatMap(([label, p]) =>
    cross(`polygon/${label}`, "polygon", (o) => [p, o], withSweep(seedRoughness(), OPTION_SWEEP)),
  ),
  ...Object.entries(RECTS).flatMap(([label, r]) =>
    cross(`rectangle/${label}`, "rectangle", (o) => [...r, o], seedRoughness()),
  ),
  ...Object.entries(CURVES).flatMap(([label, p]) =>
    cross(
      `curve/${label}`,
      "curve",
      (o) => [p, o],
      withSweep(seedRoughness(), [...OPTION_SWEEP, ["tightness0.5", { curveTightness: 0.5 }]]),
    ),
  ),
  // seed 0 makes rough.js fall back to Math.random: only the structure is reproducible.
  { name: "seed0/line", call: "line", args: [...LINES.mid, { seed: 0, roughness: 1 }] },
  { name: "seed0/rectangle", call: "rectangle", args: [...RECTS.normal, { seed: 0, roughness: 1 }] },
];

const outlineElliptic = [
  ...Object.entries(ELLIPSES).flatMap(([label, e]) =>
    cross(`ellipse/${label}`, "ellipse", (o) => [...e, o], withSweep(seedRoughness(), ELLIPTIC_SWEEP)),
  ),
  ...cross("circle", "circle", (o) => [50, 50, 90, o], seedRoughness()),
  ...Object.entries(ARCS).flatMap(([label, a]) =>
    cross(`arc/${label}`, "arc", (o) => [...a, o], withSweep(seedRoughness(), ELLIPTIC_SWEEP)),
  ),
];

const outlinePath = [
  ...Object.entries(PATHS).flatMap(([label, d]) =>
    cross(
      `path/${label}`,
      "path",
      (o) => [d, o],
      withSweep(seedRoughness(), [
        ["simplification0.5", { simplification: 0.5 }],
        ["singleStroke", { disableMultiStroke: true }],
        ["preserveVertices", { preserveVertices: true }],
      ]),
    ),
  ),
  ...Object.entries(BAD_PATHS).map(([label, d]) => ({ name: `path/${label}`, call: "path", args: [d, { seed: 1 }] })),
  { name: "path/empty", call: "path", args: ["", { seed: 1 }] },
  { name: "path/whitespace", call: "path", args: ["   ", { seed: 1 }] },
];

// Pattern fills emit a stroke per hachure line, so fill cases use small shapes: the
// generated JSON stays a few hundred KiB per style while every code path still runs.
const FILL_SHAPES = {
  rectangle: ["rectangle", (o) => [5, 10, 60, 40, o]],
  "polygon/concave": ["polygon", (o) => [[[0, 0], [60, 0], [60, 45], [30, 18], [0, 45]], o]],
  ellipse: ["ellipse", (o) => [35, 25, 60, 40, o]],
  "arc/closed": ["arc", (o) => [35, 25, 60, 40, -Math.PI / 2, Math.PI, true, o]],
  "curve/wave": ["curve", (o) => [[[0, 0], [15, 30], [35, -10], [55, 25], [70, 0]], o]],
  "path/roundedRect": ["path", (o) => ["M 10 0 L 50 0 Q 60 0, 60 10 L 60 30 Q 60 40, 50 40 L 10 40 Q 0 40, 0 30 L 0 10 Q 0 0, 10 0", o]],
  "path/subpaths": ["path", (o) => ["M 0 0 L 25 0 L 25 25 Z M 35 0 L 60 0 L 48 25 Z", o]],
};

/** Every fillable shape × seed × roughness, plus `sweep` variants, with `fillOptions` merged in. */
function fillable(fillOptions, sweep, shapes = FILL_SHAPES) {
  const cases = [];
  for (const [label, [call, argsFor]] of Object.entries(shapes)) {
    for (const [variant, o] of withSweep(seedRoughness({}, FILL_SEEDS), sweep)) {
      cases.push({ name: `${label}/${variant}`, call, args: argsFor({ fill: "#e03131", ...fillOptions, ...o }) });
    }
  }
  return cases;
}

/** Cases where the fill branch is skipped or special-cased; one style is enough. */
const FILL_EDGES = [
  { name: "edge/circle", call: "circle", args: [30, 30, 50, { seed: 1, roughness: 1, fill: "#e03131", fillStyle: "hachure", hachureGap: 8 }] },
  { name: "edge/arcOpen", call: "arc", args: [35, 25, 60, 40, 0, Math.PI, false, { seed: 1, roughness: 1, fill: "#e03131", fillStyle: "hachure" }] },
  { name: "edge/curveTwoPoints", call: "curve", args: [[[0, 0], [60, 30]], { seed: 1, roughness: 1, fill: "#e03131", fillStyle: "hachure" }] },
  { name: "edge/pathTransparent", call: "path", args: ["M 0 0 L 60 0 L 60 40 Z", { seed: 1, roughness: 1, fill: "transparent", fillStyle: "hachure" }] },
  { name: "edge/emptyFillString", call: "rectangle", args: [0, 0, 60, 40, { seed: 1, roughness: 1, fill: "", fillStyle: "hachure" }] },
  { name: "edge/noStroke", call: "rectangle", args: [0, 0, 60, 40, { seed: 1, roughness: 1, fill: "#e03131", fillStyle: "hachure", hachureGap: 8, stroke: "none" }] },
  { name: "edge/unknownStyle", call: "rectangle", args: [0, 0, 60, 40, { seed: 1, roughness: 1, fill: "#e03131", fillStyle: "bogus", hachureGap: 8 }] },
];

const PATTERN_SWEEP = [
  ["hachureAngle0", { hachureAngle: 0 }],
  ["hachureGap12", { hachureGap: 12 }],
  ["fillWeight3", { fillWeight: 3 }],
  ["singleStrokeFill", { disableMultiStrokeFill: true }],
];

export const groups = {
  random,
  path_data: pathData,
  points_on_curve: pointsOnCurve,
  points_on_path: pointsOnPath,
  hachure_fill: hachureFill,
  outline_linear: outlineLinear,
  outline_elliptic: outlineElliptic,
  outline_path: outlinePath,
  fill_solid: [
    ...fillable({ fillStyle: "solid" }, [["gain0", { fillShapeRoughnessGain: 0 }]]),
    // No subpaths at all: the multi-set branch runs solidFillPolygon on an empty list.
    { name: "path/whitespace", call: "path", args: ["   ", { seed: 1, fill: "#f00", fillStyle: "solid" }] },
  ],
  fill_hachure: [...fillable({ fillStyle: "hachure", hachureGap: 8 }, PATTERN_SWEEP), ...FILL_EDGES],
  fill_cross_hatch: fillable({ fillStyle: "cross-hatch", hachureGap: 8 }, PATTERN_SWEEP),
  fill_zigzag: fillable({ fillStyle: "zigzag", hachureGap: 8 }, PATTERN_SWEEP),
  // Dot positions come from Math.random, so these are structure-only; two shapes suffice.
  fill_dots: fillable({ fillStyle: "dots", hachureGap: 12 }, [], {
    rectangle: FILL_SHAPES.rectangle,
    ellipse: FILL_SHAPES.ellipse,
  }),
  fill_dashed: fillable({ fillStyle: "dashed", hachureGap: 8 }, [...PATTERN_SWEEP, ["dash", { dashOffset: 3, dashGap: 5 }]]),
  fill_zigzag_line: fillable({ fillStyle: "zigzag-line", hachureGap: 8 }, [
    ...PATTERN_SWEEP,
    ["zigzagOffset3", { zigzagOffset: 3 }],
  ]),
};
