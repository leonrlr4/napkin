mod support;

use scene::editor::{EdgeStyle, Property, Section, StrokeWidth, Tool};
use scene::sample::{self, CharWidthMeasure};
use serde_json::json;
use support::*;

#[test]
fn panel_is_hidden_with_nothing_selected_under_the_selection_tool() {
    let e = editor(vec![]);
    assert!(e.panel().sections.is_empty());
}

#[test]
fn creation_tools_show_their_own_sections_with_item_style_values() {
    let mut e = editor(vec![]);
    e.set_tool(Tool::Arrow);
    let panel = e.panel();
    assert!(panel.sections.contains(&Section::Arrowheads));
    assert!(!panel.sections.contains(&Section::BackgroundColor));
    assert_eq!(panel.end_arrowhead, Some(Some("arrow".to_string())));
    e.set_tool(Tool::Text);
    let panel = e.panel();
    assert!(panel.sections.contains(&Section::FontSize));
    assert_eq!(panel.font_size, Some(20.0));
}

#[test]
fn mixed_values_read_as_none_and_setting_one_is_one_undo_step() {
    let mut e = editor(vec![
        sample::with(
            sample::generic("rectangle", "a", [0.0, 0.0, 10.0, 10.0]),
            json!({"strokeColor": "#e03131"}),
        ),
        sample::generic("ellipse", "b", [20.0, 0.0, 10.0, 10.0]),
    ]);
    e.command(scene::editor::Command::SelectAll);
    assert_eq!(e.panel().stroke_color, None);
    let revision = e.revision();
    assert!(e.set_property(
        Property::StrokeColor("#1971c2".into()),
        &mut CharWidthMeasure
    ));
    assert_eq!(e.revision(), revision + 1);
    assert_eq!(e.panel().stroke_color, Some("#1971c2".into()));
    assert_eq!(e.style().stroke_color, "#1971c2");
    assert!(e.command(scene::editor::Command::Undo));
    assert_eq!(e.panel().stroke_color, None);
}

#[test]
fn setting_the_current_value_changes_nothing_but_still_updates_item_style() {
    let mut e = editor(vec![sample::generic(
        "rectangle",
        "a",
        [0.0, 0.0, 10.0, 10.0],
    )]);
    e.command(scene::editor::Command::SelectAll);
    let before = e.file().clone();
    assert!(!e.set_property(
        Property::StrokeWidth(StrokeWidth::Medium),
        &mut CharWidthMeasure
    ));
    assert_eq!(e.file(), &before);
    e.set_property(Property::Edges(EdgeStyle::Sharp), &mut CharWidthMeasure);
    assert_eq!(e.file().elements[0].to_value()["roundness"], json!(null));
    assert_eq!(e.style().edges, EdgeStyle::Sharp);
}

#[test]
fn font_size_remeasures_text_and_recenters_labels() {
    let mut e = editor(vec![
        sample::with(
            sample::generic("rectangle", "r", [0.0, 0.0, 200.0, 100.0]),
            json!({"boundElements": [{"id": "t", "type": "text"}]}),
        ),
        sample::with(
            sample::text("t", [76.0, 37.5, 48.0, 25.0], "box", Some("r")),
            json!({"textAlign": "center", "verticalAlign": "middle"}),
        ),
    ]);
    e.command(scene::editor::Command::SelectAll);
    assert!(e.set_property(Property::FontSize(40.0), &mut CharWidthMeasure));
    let label = e.file().elements[1].to_value();
    assert_eq!(label["fontSize"], json!(40.0));
    assert_eq!(label["width"], json!(3.0 * 40.0 * 0.6));
    assert_eq!(label["height"], json!(50.0));
    assert_eq!(label["x"], json!(5.0 + (190.0 / 2.0 - 72.0 / 2.0)));
    assert_eq!(label["y"], json!(5.0 + (90.0 / 2.0 - 25.0)));
}

#[test]
fn raw_elements_are_left_alone() {
    let mut e = editor(vec![
        json!({"id": "i", "type": "image", "x": 0, "y": 0, "width": 5,
        "height": 5, "isDeleted": false, "version": 1, "versionNonce": 1, "strokeColor": "#000"}),
    ]);
    e.command(scene::editor::Command::SelectAll);
    let before = e.file().clone();
    assert!(!e.set_property(
        Property::StrokeColor("#e03131".into()),
        &mut CharWidthMeasure
    ));
    assert_eq!(e.file(), &before);
}

// --- Additional coverage beyond the brief's Step-1 tests -----------------------------------

/// Section::Edges is `canChangeRoundness` narrowed to napkin's kinds (rectangle, diamond,
/// line): an ellipse gets no edges control, unlike the brief's own summary (see
/// `properties.rs`'s doc comment for the JS source backing this).
#[test]
fn edges_section_excludes_ellipse() {
    let mut e = editor(vec![sample::generic(
        "ellipse",
        "e",
        [0.0, 0.0, 10.0, 10.0],
    )]);
    e.command(scene::editor::Command::SelectAll);
    assert!(!e.panel().sections.contains(&Section::Edges));

    let mut e = editor(vec![sample::generic(
        "rectangle",
        "r",
        [0.0, 0.0, 10.0, 10.0],
    )]);
    e.command(scene::editor::Command::SelectAll);
    assert!(e.panel().sections.contains(&Section::Edges));
}

/// `actionChangeSloppiness` re-rolls the element's seed alongside its roughness.
#[test]
fn roughness_reseeds_the_element() {
    let mut e = editor(vec![sample::generic(
        "rectangle",
        "r",
        [0.0, 0.0, 10.0, 10.0],
    )]);
    e.command(scene::editor::Command::SelectAll);
    let seed_before = e.file().elements[0].to_value()["seed"].clone();
    assert!(e.set_property(Property::Roughness(2.0), &mut CharWidthMeasure));
    let value = e.file().elements[0].to_value();
    assert_eq!(value["roughness"], json!(2.0));
    assert_ne!(value["seed"], seed_before);
}

/// `changeProperty`'s callback for `BackgroundColor`/`StrokeWidth`/`StrokeStyle`/`Roughness`/
/// `Edges`/`Opacity` has no type filter in JS: it also writes to a selected arrow's unused
/// `backgroundColor` field, unlike `FillStyle` which is filtered by `hasFillStyle`.
#[test]
fn background_color_applies_even_to_a_kind_without_a_background_section() {
    let mut e = editor(vec![sample::linear(
        "arrow",
        "a",
        [0.0, 0.0],
        &[[0.0, 0.0], [10.0, 0.0]],
    )]);
    e.command(scene::editor::Command::SelectAll);
    assert!(e.set_property(
        Property::BackgroundColor("#e03131".into()),
        &mut CharWidthMeasure
    ));
    assert_eq!(
        e.file().elements[0].to_value()["backgroundColor"],
        json!("#e03131")
    );
    assert!(!e.panel().sections.contains(&Section::BackgroundColor));

    assert!(!e.set_property(
        Property::FillStyle("cross-hatch".into()),
        &mut CharWidthMeasure
    ));
}

/// Arrowheads apply to a selected line too (`isLinearElement`), even though only an arrow
/// shows the Arrowheads section.
#[test]
fn arrowhead_applies_to_a_selected_line() {
    let mut e = editor(vec![sample::linear(
        "line",
        "l",
        [0.0, 0.0],
        &[[0.0, 0.0], [10.0, 0.0]],
    )]);
    e.command(scene::editor::Command::SelectAll);
    assert!(e.set_property(
        Property::EndArrowhead(Some("triangle".into())),
        &mut CharWidthMeasure
    ));
    assert_eq!(
        e.file().elements[0].to_value()["endArrowhead"],
        json!("triangle")
    );
}

/// A standalone `autoResize` text keeps its align-appropriate edge fixed and recentres
/// vertically (`offsetElementAfterFontResize`), for each of the three alignments.
#[test]
fn standalone_font_size_keeps_the_aligned_edge_and_recenters_vertically() {
    // old width 60, new width 48 ("hi" at font size 40): dx is 0 (left), half the 12px
    // shrink (center) or the whole shrink (right, keeping the right edge fixed).
    for (align, expected_x) in [("left", 100.0), ("center", 106.0), ("right", 112.0)] {
        let mut e = editor(vec![sample::with(
            sample::text("t", [100.0, 100.0, 60.0, 25.0], "hi", None),
            json!({"textAlign": align}),
        )]);
        e.command(scene::editor::Command::SelectAll);
        assert!(e.set_property(Property::FontSize(40.0), &mut CharWidthMeasure));
        let value = e.file().elements[0].to_value();
        // "hi" is 2 chars: width = 2 * 40 * 0.6 = 48, height = 40 * 1.25 = 50.
        assert_eq!(value["width"], json!(48.0), "align {align}");
        assert_eq!(value["x"], json!(expected_x), "align {align}");
        assert_eq!(
            value["y"],
            json!(100.0 + (25.0 - 50.0) / 2.0),
            "align {align}"
        );
    }
}

/// `predicates.fill`: a transparent background (the default item style, or a selected
/// element's own) hides the FillStyle section even where `hasFillStyle` would otherwise show
/// it; a non-transparent background brings it back.
#[test]
fn fill_style_section_needs_a_non_transparent_background() {
    let mut e = editor(vec![]);
    e.set_tool(Tool::Rectangle);
    assert!(!e.panel().sections.contains(&Section::FillStyle));

    e.set_property(
        Property::BackgroundColor("#a5d8ff".into()),
        &mut CharWidthMeasure,
    );
    assert!(e.panel().sections.contains(&Section::FillStyle));

    let mut e = editor(vec![sample::generic(
        "rectangle",
        "r",
        [0.0, 0.0, 10.0, 10.0],
    )]);
    e.command(scene::editor::Command::SelectAll);
    assert!(!e.panel().sections.contains(&Section::FillStyle));
}
