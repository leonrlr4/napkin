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
  // `end == 1`: the JS loop that searches for the farthest point runs zero times.
  { name: "simplify/onePoint", call: "simplify", args: [[[1, 1]], 0.75] },
  ...Object.entries({ ...CURVES, single: [[1, 1]] }).flatMap(([label, points]) =>
    [0, 0.5].map((tightness) => ({
      name: `curveToBezier/${label}/k${tightness}`,
      call: "curveToBezier",
      args: [points, tightness],
    })),
  ),
];

// Single-point subpaths: each produces a `currentPoints` set of length 1, so `simplify`
// runs with `end == 1`.
const SINGLE_POINT_PATHS = {
  singleMove: "M 10 10",
  singleMoveAmongOthers: "M 10 10 L 20 20 M 5 5",
};

const pointsOnPath = Object.entries({ ...PATHS, ...SINGLE_POINT_PATHS }).flatMap(([label, d]) =>
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
  // Every subpath simplifies with `end == 1`: exercises points_on_curve's one-point case
  // through RoughGenerator::path.
  ...Object.entries(SINGLE_POINT_PATHS).map(([label, d]) => ({
    name: `path/${label}`,
    call: "path",
    args: [d, { seed: 1, roughness: 1 }],
  })),
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

// --- js_math -----------------------------------------------------------------------
//
// `Math.atan2`/`Math.hypot` inputs for `js::atan2`/`js::hypot` (crates/rough/src/js.rs),
// checked bit-for-bit (crates/rough/tests/baseline.rs), not the usual 1e-9 tolerance:
// libm's `atan2`/`hypot` disagree with V8's fdlibm-derived/Torque implementations in the
// last bit often enough to change loop bounds in `scene` (see the ported functions' doc
// comments). A fixed-seed PRNG (not `Math.random`, which `harness.mjs` repoints at a
// per-case stream for the reproducibility check the other groups need) keeps this
// deterministic across regenerations.

/** mulberry32: small, seedable, good enough for test-input coverage (not cryptographic). */
function mulberry32(seed) {
  let a = seed >>> 0;
  return function () {
    a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

/** A uniformly-chosen bit pattern reinterpreted as `float64` (any exponent, any mantissa). */
function randomBits(rng) {
  const buf = new ArrayBuffer(8);
  const dv = new DataView(buf);
  dv.setUint32(0, Math.floor(rng() * 0x100000000), false);
  dv.setUint32(4, Math.floor(rng() * 0x100000000), false);
  return dv.getFloat64(0, false);
}

/** A random subnormal (biased exponent 0, non-zero mantissa): below `Number.MIN_VALUE`'s
 * normal cousin `2.2250738585072014e-308`. */
function randomSubnormal(rng, sign) {
  const buf = new ArrayBuffer(8);
  const dv = new DataView(buf);
  const hi = Math.floor(rng() * 0x100000);
  const lo = Math.floor(rng() * 0x100000000);
  dv.setUint32(0, (sign < 0 ? 0x80000000 : 0) | hi, false);
  dv.setUint32(4, lo, false);
  return dv.getFloat64(0, false);
}

/**
 * One operand for a `js_math` case: a mix of ordinary values, tiny/huge magnitudes, exact
 * zero (both signs), subnormals, infinities, NaN and fully random bit patterns, so that
 * 20,000 draws cover every distinct branch `atan2`/`hypot` take on operand magnitude and
 * sign, not just a uniform range.
 */
function sampleDouble(rng) {
  const bucket = Math.floor(rng() * 10);
  const sign = rng() < 0.5 ? -1 : 1;
  switch (bucket) {
    case 0:
      return sign * 0; // +0 / -0
    case 1:
      return sign * rng() * 1000; // ordinary range
    case 2:
      return sign * rng() * 1e6; // ordinary, wider range
    case 3: { // tiny normal magnitude
      const exp = -1 - Math.floor(rng() * 300);
      return sign * rng() * 10 ** exp;
    }
    case 4: { // huge magnitude, up to ~1e308
      const exp = 1 + Math.floor(rng() * 307);
      return sign * (1 + rng()) * 10 ** exp;
    }
    case 5:
      return randomSubnormal(rng, sign);
    case 6:
      return sign * Infinity;
    case 7:
      return NaN;
    case 8:
      return sign * (1 + (rng() - 0.5) * 0.01); // near 1: atan2's `x == 1` fast path, atan's interval boundaries
    default:
      return randomBits(rng); // arbitrary bit pattern, including further NaNs/infinities
  }
}

function randomMathCases(name, call, count, seed) {
  const rng = mulberry32(seed);
  const cases = [];
  for (let i = 0; i < count; i++) {
    const a = sampleDouble(rng);
    const b = sampleDouble(rng);
    cases.push({ name: `${name}/r${String(i).padStart(6, "0")}`, call, args: [a, b] });
  }
  return cases;
}

const ATAN2_EDGES = [
  [0, 0], [-0, 0], [0, -0], [-0, -0],
  [1, 0], [-1, 0], [0, 1], [0, -1], [-0, 1], [-0, -1],
  [1, 1], [1, -1], [-1, 1], [-1, -1],
  [Infinity, Infinity], [Infinity, -Infinity], [-Infinity, Infinity], [-Infinity, -Infinity],
  [Infinity, 5], [5, Infinity], [-Infinity, 5], [5, -Infinity],
  [Infinity, 0], [0, Infinity], [-Infinity, 0], [0, -Infinity],
  [NaN, 1], [1, NaN], [NaN, NaN], [NaN, Infinity],
  [5, 1], [1, 1], [2, 1], // atan2(y,1): the `x === 1.0` fast path
  [1e-320, 1e-320], [1e308, 1e308], [1e-320, -1e-320], [1e308, -1e308],
  // Task 11 fix-round-1 finding 2's repro (laser-pointer corner angle, constant stroke
  // [[0,0],[-2,-5]], strokeWidth 2, streamline 0.5): V8 and glibc `atan2` disagree in the
  // last bit here, which used to change an outline point count.
  [-2.5, -1],
];

const HYPOT_EDGES = [
  [0, 0], [-0, 0], [0, -0], [-0, -0],
  [1, 0], [0, 1], [-1, -1],
  [Infinity, 5], [5, Infinity], [-Infinity, 5], [5, -Infinity], [Infinity, NaN], [NaN, Infinity],
  [NaN, 5], [5, NaN], [NaN, NaN], [-Infinity, -Infinity],
  [1e300, 1e300], [1e-300, 1e-300], [1e308, 1e308], [5e-324, 5e-324],
  // perfect-freehand's `dist` (Task 11 fix-round-1 finding 2's repro): points
  // [0,0], [0,0], [-0.1547364747990101,-2.9960067795929257], streamline 1, width 0.5.
  [-0.1547364747990101, -2.9960067795929257],
];

const jsMath = [
  ...ATAN2_EDGES.map(([y, x], i) => ({ name: `atan2/edge${i}`, call: "atan2", args: [y, x] })),
  ...randomMathCases("atan2", "atan2", 20000, 1),
  ...HYPOT_EDGES.map(([x, y], i) => ({ name: `hypot/edge${i}`, call: "hypot", args: [x, y] })),
  ...randomMathCases("hypot", "hypot", 20000, 2),
];

export const groups = {
  random,
  js_math: jsMath,
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
    // Multiple subpaths, one of them single-point: covers the multi-subpath solid branch.
    {
      name: "path/singleMoveAmongOthers",
      call: "path",
      args: [SINGLE_POINT_PATHS.singleMoveAmongOthers, { seed: 1, roughness: 1, fill: "#f00", fillStyle: "solid" }],
    },
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
