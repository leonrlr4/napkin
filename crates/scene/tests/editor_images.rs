mod support;

use scene::editor::{Command, Tool};
use scene::file::FileData;
use scene::image::{PreparedImage, natural_placement};
use serde_json::json;
use support::*;

fn png(id: &str, natural: [f64; 2]) -> PreparedImage {
    PreparedImage {
        file: FileData {
            id: id.into(),
            mime_type: "image/png".into(),
            data_url: "data:image/png;base64,AAAA".into(),
            created: 1.0,
            last_retrieved: 1.0,
        },
        natural_size: natural,
    }
}

#[test]
fn natural_placement_fits_tall_images_to_half_the_canvas() {
    // Canvas 900 tall at zoom 1: min(780, 450) = 450 is the height cap.
    assert_eq!(
        natural_placement([500.0, 500.0], [400.0, 300.0], 900.0, 1.0),
        [300.0, 350.0, 400.0, 300.0]
    );
    assert_eq!(
        natural_placement([500.0, 500.0], [2000.0, 1000.0], 900.0, 1.0),
        [50.0, 275.0, 900.0, 450.0]
    );
}

#[test]
fn inserting_an_image_adds_its_file_selects_it_and_undoes_in_one_step() {
    let mut e = editor(vec![]);
    assert!(e.insert_images(vec![png("f1", [400.0, 300.0])], [500.0, 500.0], 900.0, 1.0));
    let v = e.file().elements[0].to_value();
    assert_eq!(v["type"], json!("image"));
    assert_eq!(v["fileId"], json!("f1"));
    assert_eq!(v["status"], json!("saved"));
    assert_eq!(
        [
            v["x"].clone(),
            v["y"].clone(),
            v["width"].clone(),
            v["height"].clone()
        ],
        [json!(300.0), json!(350.0), json!(400.0), json!(300.0)]
    );
    assert!(e.file().file_data("f1").is_some());
    assert_eq!(selected(&e), [v["id"].as_str().unwrap()]);
    assert_eq!(e.tool(), Tool::Selection);
    assert!(e.command(Command::Undo));
    assert!(e.file().elements.is_empty());
    assert!(
        e.file().file_data("f1").is_some(),
        "files are not part of undo"
    );
}

#[test]
fn copying_an_image_carries_its_file_into_the_paste() {
    let mut e = editor(vec![]);
    e.insert_images(vec![png("f1", [100.0, 100.0])], [0.0, 0.0], 900.0, 1.0);
    let text = e.copy_selection().expect("something selected");
    let json: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        json["files"]["f1"]["dataURL"],
        json!("data:image/png;base64,AAAA")
    );

    let mut other = editor(vec![]);
    assert!(other.paste(&text, [50.0, 50.0], &mut scene::sample::CharWidthMeasure));
    assert!(other.file().file_data("f1").is_some());
}

#[test]
fn inserting_several_images_lays_them_on_a_grid_as_one_undo_step() {
    let mut e = editor(vec![]);
    assert!(!e.insert_images(vec![], [0.0, 0.0], 900.0, 1.0));
    assert!(e.insert_images(
        vec![png("a", [100.0, 100.0]), png("b", [100.0, 100.0])],
        [0.0, 0.0],
        900.0,
        2.0,
    ));
    let xs: Vec<f64> = e
        .file()
        .elements
        .iter()
        .map(|el| el.to_value()["x"].as_f64().unwrap())
        .collect();
    // Two 100-wide cells, 25 apart (50 / zoom), centered on x = 0.
    assert_eq!(xs, [-112.5, 12.5]);
    assert_eq!(selected(&e).len(), 2);
    assert!(e.command(Command::Undo));
    assert!(e.file().elements.is_empty());
}

#[test]
fn paste_only_adds_files_that_pasted_images_use() {
    let mut e = editor(vec![]);
    e.insert_images(vec![png("f1", [100.0, 100.0])], [0.0, 0.0], 900.0, 1.0);
    let mut payload: serde_json::Value =
        serde_json::from_str(&e.copy_selection().unwrap()).unwrap();
    payload["files"]["stray"] = payload["files"]["f1"].clone();
    payload["files"]["stray"]["id"] = json!("stray");

    let mut other = editor(vec![]);
    assert!(other.paste(
        &payload.to_string(),
        [0.0, 0.0],
        &mut scene::sample::CharWidthMeasure
    ));
    assert!(other.file().file_data("f1").is_some());
    assert!(other.file().file_data("stray").is_none());
}

#[test]
fn a_deleted_image_leaves_no_files_entry_in_the_saved_text_until_undone() {
    let mut e = editor(vec![]);
    e.insert_images(vec![png("f1", [100.0, 100.0])], [0.0, 0.0], 900.0, 1.0);
    let saved = |e: &scene::editor::Editor<_>| -> serde_json::Value {
        serde_json::from_str(&e.file().to_json_string()).unwrap()
    };
    assert!(saved(&e)["files"]["f1"].is_object());

    assert!(e.command(Command::Delete));
    assert!(saved(&e)["files"].as_object().unwrap().is_empty());
    assert!(e.file().file_data("f1").is_some(), "memory keeps the file");

    assert!(e.command(Command::Undo));
    assert!(saved(&e)["files"]["f1"].is_object());
}
