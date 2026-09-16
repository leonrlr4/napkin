//! A deterministic performance-test scene (spec §6.7, decision 11): a large, reproducible mix
//! of element kinds and fill styles spread over a 4000 x 3000 unit area, used by `--bench` and
//! `examples/perf_fixture.rs`.

use serde_json::{Value, json};

use crate::sample;

/// Scene extent: elements are placed within `0..SCENE_WIDTH` x `0..SCENE_HEIGHT`.
const SCENE_WIDTH: f64 = 4000.0;
const SCENE_HEIGHT: f64 = 3000.0;

/// `backgroundColor` / `strokeColor` candidates: Excalidraw's default swatch colors, including
/// `transparent` so some shapes have no fill.
const BACKGROUND_COLORS: [&str; 6] = [
    "transparent",
    "#ffc9c9",
    "#b2f2bb",
    "#a5d8ff",
    "#ffec99",
    "#eebefa",
];
const STROKE_COLORS: [&str; 5] = ["#1e1e1e", "#e03131", "#2f9e44", "#1971c2", "#f08c00"];
const FILL_STYLES: [&str; 4] = ["hachure", "cross-hatch", "solid", "zigzag"];

/// A `Numerical Recipes` linear congruential generator: `state = state * 1664525 + 1013904223`
/// (wrapping `u32` arithmetic), the same constants the brief specifies.
struct Lcg(u32);

impl Lcg {
    fn next_u32(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1664525).wrapping_add(1013904223);
        self.0
    }

    /// A value in `[0, 1)`.
    fn next_f64(&mut self) -> f64 {
        f64::from(self.next_u32()) / (f64::from(u32::MAX) + 1.0)
    }

    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + self.next_f64() * (hi - lo)
    }

    /// A value in `0..bound`.
    fn index(&mut self, bound: usize) -> usize {
        (self.next_u32() as usize) % bound
    }
}

/// A deterministic scene: 45% rectangles/diamonds/ellipses, 25% lines/arrows, 15% freedraw,
/// 10% text, 5% at opacity 50; every fill style; spread over 4000 x 3000 units.
///
/// Element type is chosen from a roll over the first four percentages (which sum to 95, not
/// 100); the 5% opacity-50 rate is an independent roll applied on top of whatever type was
/// picked, not a fifth type bucket.
pub fn generate(seed: u32, count: usize) -> scene::SceneFile {
    let mut lcg = Lcg(seed);
    let mut values: Vec<Value> = Vec::with_capacity(count);
    for i in 0..count {
        let id = format!("e{i}");
        let element_seed = lcg.next_u32();
        let type_roll = lcg.index(95);
        let mut value = if type_roll < 45 {
            generic_element(&mut lcg, &id)
        } else if type_roll < 70 {
            linear_element(&mut lcg, &id)
        } else if type_roll < 85 {
            freedraw_element(&mut lcg, &id)
        } else {
            text_element(&mut lcg, &id, i)
        };
        value = sample::with(value, json!({ "seed": element_seed }));
        if lcg.index(100) < 5 {
            value = sample::with(value, json!({ "opacity": 50 }));
        }
        values.push(value);
    }

    let keys = scene::fractional_index::generate_n_keys_between(None, None, count)
        .expect("None..None always has room for any count of keys");
    for (value, key) in values.iter_mut().zip(keys) {
        value["index"] = json!(key);
    }

    sample::file(values)
}

fn generic_element(lcg: &mut Lcg, id: &str) -> Value {
    let kind = ["rectangle", "diamond", "ellipse"][lcg.index(3)];
    let width = lcg.range(10.0, 300.0);
    let height = lcg.range(10.0, 300.0);
    let x = lcg.range(0.0, SCENE_WIDTH - width);
    let y = lcg.range(0.0, SCENE_HEIGHT - height);
    let value = sample::generic(kind, id, [x, y, width, height]);
    sample::with(
        value,
        json!({
            "fillStyle": FILL_STYLES[lcg.index(FILL_STYLES.len())],
            "backgroundColor": BACKGROUND_COLORS[lcg.index(BACKGROUND_COLORS.len())],
            "strokeColor": STROKE_COLORS[lcg.index(STROKE_COLORS.len())],
        }),
    )
}

fn linear_element(lcg: &mut Lcg, id: &str) -> Value {
    let kind = if lcg.index(2) == 0 { "line" } else { "arrow" };
    let x = lcg.range(0.0, SCENE_WIDTH - 200.0);
    let y = lcg.range(0.0, SCENE_HEIGHT - 200.0);
    let point_count = 2 + lcg.index(3);
    let mut points = vec![[0.0, 0.0]];
    for _ in 1..point_count {
        points.push([lcg.range(0.0, 150.0), lcg.range(0.0, 150.0)]);
    }
    let value = sample::linear(kind, id, [x, y], &points);
    sample::with(
        value,
        json!({ "strokeColor": STROKE_COLORS[lcg.index(STROKE_COLORS.len())] }),
    )
}

fn freedraw_element(lcg: &mut Lcg, id: &str) -> Value {
    let x = lcg.range(0.0, SCENE_WIDTH - 100.0);
    let y = lcg.range(0.0, SCENE_HEIGHT - 100.0);
    let point_count = 4 + lcg.index(8);
    let mut points = vec![[0.0, 0.0]];
    for _ in 1..point_count {
        points.push([lcg.range(0.0, 80.0), lcg.range(0.0, 80.0)]);
    }
    let value = sample::freedraw(id, [x, y], &points);
    sample::with(
        value,
        json!({ "strokeColor": STROKE_COLORS[lcg.index(STROKE_COLORS.len())] }),
    )
}

fn text_element(lcg: &mut Lcg, id: &str, index: usize) -> Value {
    let width = lcg.range(40.0, 200.0);
    let height = lcg.range(20.0, 40.0);
    let x = lcg.range(0.0, SCENE_WIDTH - width);
    let y = lcg.range(0.0, SCENE_HEIGHT - height);
    let value = sample::text(
        id,
        [x, y, width, height],
        &format!("napkin fixture {index}"),
        None,
    );
    sample::with(
        value,
        json!({ "strokeColor": STROKE_COLORS[lcg.index(STROKE_COLORS.len())] }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_and_typed() {
        let a = generate(7, 1000);
        assert_eq!(a.elements.len(), 1000);
        assert_eq!(a.to_json_string(), generate(7, 1000).to_json_string());
        assert!(
            a.elements
                .iter()
                .all(|e| !matches!(e, scene::Element::Raw(_)))
        );
        let translucent = a.elements.iter().filter(|e| e.opacity() < 100.0).count();
        assert!((30..=70).contains(&translucent), "{translucent}");
    }
}
