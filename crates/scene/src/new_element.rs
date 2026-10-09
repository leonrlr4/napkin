//! `packages/element/src/newElement.ts` (`_newElementBase`, the per-type constructors and
//! `newTextElement`'s `getTextAnchorRatios`/`getTextElementPositionOffsets`) and `bumpVersion`
//! from `mutateElement.ts`. Defaults are the constructors' own, not the editor's current-item
//! settings; the tools in M4 pass those in `ElementProps`.

use serde_json::{Map, Value, json};

use crate::element::{
    Element, ElementBase, FreedrawElement, GenericElement, ImageElement, LinearElement, Roundness,
    StrokeOptions, TextElement,
};
use crate::env::{Env, random_id, random_integer};
use crate::json::Slot;
use crate::text::{self, TextMeasure};

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

/// `newTextElement`'s type-specific options; the shared ones are in `ElementProps`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextProps {
    pub text: String,
    /// `None` (or a JS-falsy value) takes `newTextElement`'s default.
    pub font_size: Option<f64>,
    pub font_family: Option<f64>,
    pub text_align: Option<String>,
    pub vertical_align: Option<String>,
    pub container_id: Option<String>,
    pub line_height: Option<f64>,
}

/// JS `opts.field || default`, applied to a string: `None` and `Some(String::new())` (an empty
/// string, JS-falsy) both mean "not given".
fn non_empty(value: Option<String>) -> Option<String> {
    value.filter(|s| !s.is_empty())
}

/// `newTextElement`. `fontFamily`, `fontSize` and `lineHeight` fall back on JS-falsy input
/// (`0`, `NaN`, missing), matching the JS `||`; `textAlign`/`verticalAlign` do the same for an
/// empty string.
pub fn new_text_element(
    props: ElementProps,
    text: TextProps,
    measure: &mut dyn TextMeasure,
    env: &mut impl Env,
) -> Element {
    let font_family = text
        .font_family
        .filter(|&f| rough::js::truthy(f))
        .unwrap_or(text::DEFAULT_FONT_FAMILY);
    let font_size = text
        .font_size
        .filter(|&f| rough::js::truthy(f))
        .unwrap_or(text::DEFAULT_FONT_SIZE);
    let line_height = text
        .line_height
        .filter(|&f| rough::js::truthy(f))
        .unwrap_or_else(|| text::line_height(font_family));
    let normalized = text::normalize_text(&text.text);
    let [width, height] =
        text::measure_text(&normalized, font_family, font_size, line_height, measure);
    let text_align = non_empty(text.text_align).unwrap_or_else(|| "left".to_owned());
    let vertical_align = non_empty(text.vertical_align).unwrap_or_else(|| "top".to_owned());
    let ratio_x = match text_align.as_str() {
        "center" => 0.5,
        "right" => 1.0,
        _ => 0.0,
    };
    let ratio_y = match vertical_align.as_str() {
        "middle" => 0.5,
        "bottom" => 1.0,
        _ => 0.0,
    };
    let x = props.x - width * ratio_x;
    let y = props.y - height * ratio_y;
    let (base, mut extra) = new_base(
        "text",
        ElementProps {
            x,
            y,
            width,
            height,
            ..props
        },
        env,
    );
    extra.insert("baseFontSize".into(), Value::Null);
    extra.insert("labelPosition".into(), Value::Null);
    Element::Text(TextElement {
        base,
        text: normalized.clone(),
        font_size,
        font_family,
        text_align,
        vertical_align,
        container_id: non_empty(text.container_id).map_or(Slot::Null, Slot::Value),
        original_text: Some(normalized),
        auto_resize: Some(true),
        line_height: Some(line_height),
        extra,
    })
}

/// `newImageElement`'s type-specific options; the shared ones are in `ElementProps`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ImageProps {
    pub file_id: Option<String>,
    pub status: Option<String>,
    pub scale: Option<[f64; 2]>,
}

/// `newImageElement`. `strokeColor` is always `"transparent"` whatever `props` says; `status`
/// defaults to `"pending"`, `fileId` to `null`, `scale` to `[1, 1]` and `crop` to `null`.
pub fn new_image_element(props: ElementProps, image: ImageProps, env: &mut impl Env) -> Element {
    let (mut base, extra) = new_base("image", props, env);
    base.stroke_color = "transparent".into();
    Element::Image(ImageElement {
        base,
        file_id: image.file_id.map_or(Slot::Null, Slot::Value),
        status: image.status.unwrap_or_else(|| "pending".to_owned()),
        scale: Some(image.scale.unwrap_or([1.0, 1.0])),
        crop: Slot::Null,
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

    #[test]
    fn new_image_element_round_trips_through_json() {
        let mut env = TestEnv { now: 100.0 };
        let element = new_image_element(ElementProps::default(), ImageProps::default(), &mut env);
        let reloaded = Element::from_value(element.to_value());
        assert_eq!(reloaded, element);
        assert!(matches!(reloaded, Element::Image(_)));
    }

    #[test]
    fn new_text_element_round_trips_through_json() {
        let mut env = TestEnv { now: 100.0 };
        let element = new_text_element(
            ElementProps::default(),
            TextProps {
                text: "hello".into(),
                ..TextProps::default()
            },
            &mut crate::sample::CharWidthMeasure,
            &mut env,
        );
        assert!(matches!(element, Element::Text(_)));
        let reloaded = Element::from_value(element.to_value());
        assert!(
            matches!(reloaded, Element::Text(_)),
            "new_text_element's own output must load back as a typed element, not fall back to Raw"
        );
    }
}
