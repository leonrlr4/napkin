//! `packages/element/src/newElement.ts` (`_newElementBase` and the per-type constructors)
//! and `bumpVersion` from `mutateElement.ts`. Defaults are the constructors' own, not the
//! editor's current-item settings; the tools in M4 pass those in `ElementProps`.

use serde_json::{Map, Value, json};

use crate::element::{
    Element, ElementBase, FreedrawElement, GenericElement, LinearElement, Roundness, StrokeOptions,
};
use crate::env::{Env, random_id, random_integer};
use crate::json::Slot;

/// `DEFAULT_STROKE_STREAMLINE` (packages/common/src/constants.ts).
pub const DEFAULT_STROKE_STREAMLINE: f64 = 0.5;

#[derive(Clone, Debug, PartialEq)]
pub struct ElementProps {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub angle: f64,
    pub stroke_color: String,
    pub background_color: String,
    pub fill_style: String,
    pub stroke_width: f64,
    pub stroke_style: String,
    pub roughness: f64,
    pub opacity: f64,
    pub group_ids: Vec<String>,
    pub roundness: Option<Roundness>,
    pub locked: bool,
}

impl Default for ElementProps {
    /// `DEFAULT_ELEMENT_PROPS` plus `_newElementBase`'s parameter defaults.
    fn default() -> Self {
        ElementProps {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
            angle: 0.0,
            stroke_color: "#1e1e1e".into(),
            background_color: "transparent".into(),
            fill_style: "solid".into(),
            stroke_width: 2.0,
            stroke_style: "solid".into(),
            roughness: 1.0,
            opacity: 100.0,
            group_ids: Vec::new(),
            roundness: None,
            locked: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GenericKind {
    Rectangle,
    Diamond,
    Ellipse,
}

/// `_newElementBase`: the shared fields, plus the untyped ones for `extra`.
fn new_base(
    kind: &str,
    props: ElementProps,
    env: &mut impl Env,
) -> (ElementBase, Map<String, Value>) {
    let timestamp = env.now_ms();
    let base = ElementBase {
        id: random_id(env),
        kind: kind.into(),
        x: props.x,
        y: props.y,
        width: props.width,
        height: props.height,
        angle: props.angle,
        stroke_color: props.stroke_color,
        background_color: props.background_color,
        fill_style: props.fill_style,
        stroke_width: props.stroke_width,
        stroke_style: props.stroke_style,
        roughness: props.roughness,
        opacity: props.opacity,
        group_ids: props.group_ids,
        index: Slot::Null,
        roundness: props.roundness.map_or(Slot::Null, Slot::Value),
        seed: random_integer(env),
        version: 1.0,
        version_nonce: 0.0,
        is_deleted: false,
        updated: Slot::Value(timestamp),
    };
    let mut extra = Map::new();
    extra.insert("frameId".into(), Value::Null);
    extra.insert("boundElements".into(), Value::Null);
    extra.insert("created".into(), json!(timestamp));
    extra.insert("link".into(), Value::Null);
    extra.insert("locked".into(), json!(props.locked));
    (base, extra)
}

pub fn new_generic_element(kind: GenericKind, props: ElementProps, env: &mut impl Env) -> Element {
    let name = match kind {
        GenericKind::Rectangle => "rectangle",
        GenericKind::Diamond => "diamond",
        GenericKind::Ellipse => "ellipse",
    };
    let (base, extra) = new_base(name, props, env);
    let element = GenericElement { base, extra };
    match kind {
        GenericKind::Rectangle => Element::Rectangle(element),
        GenericKind::Diamond => Element::Diamond(element),
        GenericKind::Ellipse => Element::Ellipse(element),
    }
}

/// `newLinearElement` for `type: "line"`. Width and height come from `props`, as in
/// Excalidraw (the constructor does not derive them from `points`).
pub fn new_line_element(props: ElementProps, points: Vec<[f64; 2]>, env: &mut impl Env) -> Element {
    let (base, mut extra) = new_base("line", props, env);
    extra.insert("startBinding".into(), Value::Null);
    extra.insert("endBinding".into(), Value::Null);
    extra.insert("polygon".into(), json!(false));
    Element::Line(LinearElement {
        base,
        points,
        start_arrowhead: Slot::Null,
        end_arrowhead: Slot::Null,
        elbowed: None,
        extra,
    })
}

/// `newArrowElement` without `elbowed` (napkin cannot create elbow arrows, spec §1.2).
pub fn new_arrow_element(
    props: ElementProps,
    points: Vec<[f64; 2]>,
    start_arrowhead: Option<String>,
    end_arrowhead: Option<String>,
    env: &mut impl Env,
) -> Element {
    let (base, mut extra) = new_base("arrow", props, env);
    extra.insert("startBinding".into(), Value::Null);
    extra.insert("endBinding".into(), Value::Null);
    Element::Arrow(LinearElement {
        base,
        points,
        start_arrowhead: start_arrowhead.map_or(Slot::Null, Slot::Value),
        end_arrowhead: end_arrowhead.map_or(Slot::Null, Slot::Value),
        elbowed: Some(false),
        extra,
    })
}

pub fn new_freedraw_element(
    props: ElementProps,
    points: Vec<[f64; 2]>,
    pressures: Vec<f64>,
    simulate_pressure: bool,
    stroke_options: Option<StrokeOptions>,
    env: &mut impl Env,
) -> Element {
    let (base, extra) = new_base("freedraw", props, env);
    let stroke_options = stroke_options.unwrap_or_else(|| StrokeOptions {
        variability: Slot::Value("variable".into()),
        streamline: Slot::Value(DEFAULT_STROKE_STREAMLINE),
        extra: Map::new(),
    });
    Element::Freedraw(FreedrawElement {
        base,
        points,
        pressures,
        simulate_pressure: Some(simulate_pressure),
        stroke_options: Slot::Value(stroke_options),
        extra,
    })
}

/// `bumpVersion`: every modification increments `version`, redraws `versionNonce` and
/// stamps `updated` (spec §5.3).
pub fn bump_version(element: &mut Element, env: &mut impl Env) {
    let nonce = random_integer(env);
    let now = env.now_ms();
    match element {
        Element::Raw(Value::Object(map)) => {
            let version = map.get("version").and_then(Value::as_f64).unwrap_or(0.0);
            map.insert("version".into(), json!(version + 1.0));
            map.insert("versionNonce".into(), json!(nonce));
            map.insert("updated".into(), json!(now));
        }
        Element::Raw(_) => {}
        _ => {
            let base = element.base_mut().expect("typed element");
            base.version += 1.0;
            base.version_nonce = nonce;
            base.updated = Slot::Value(now);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Random bytes all `0x11`, clock advancing by one on every call (unlike
    /// `tests/baseline.rs`'s `FixedEnv`, whose clock is pinned at 1ms): every value this
    /// produces is distinct, so a `bump_version` regression can't hide behind matching
    /// defaults.
    struct TestEnv {
        now: f64,
    }

    impl Env for TestEnv {
        fn fill_random(&mut self, bytes: &mut [u8]) {
            bytes.fill(0x11);
        }

        fn now_ms(&mut self) -> f64 {
            self.now += 1.0;
            self.now
        }
    }

    #[test]
    fn bump_version_updates_typed_element_fields() {
        let mut env = TestEnv { now: 100.0 };
        let mut element =
            new_generic_element(GenericKind::Rectangle, ElementProps::default(), &mut env);
        let Element::Rectangle(_) = &element else {
            panic!("expected a typed rectangle element");
        };
        let version_before = element.base().expect("typed element").version;
        // `now_ms` was called once inside `new_generic_element`; the next call, inside
        // `bump_version` below, advances it by one more.
        let expected_now = env.now + 1.0;
        // `TestEnv::fill_random` always fills the same bytes, so `random_integer` always
        // returns this value regardless of when it is called.
        let expected_nonce = f64::from(u32::from_le_bytes([0x11; 4]) >> 1);

        bump_version(&mut element, &mut env);

        let Element::Rectangle(_) = &element else {
            panic!("bump_version must not change the element's type");
        };
        let base = element.base().expect("typed element");
        assert_eq!(base.version, version_before + 1.0);
        assert_eq!(base.version_nonce, expected_nonce);
        assert_eq!(element.to_value()["updated"], json!(expected_now));
    }
}
