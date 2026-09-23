//! Spec §9.2 round trip: every file in `tests/corpus/`, drawn on excalidraw.com, must come
//! back semantically unchanged after `SceneFile` reads and writes it. A second test checks
//! the corpus still contains everything §9.2 asks for, so a thin corpus cannot pass.

use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;

use scene::json::semantic_eq;
use scene::{Element, SceneFile};
use serde_json::Value;
use testkit::{Compare, diff};

/// Element types `Element::from_value` parses into structs.
const TYPED: [&str; 7] = [
    "rectangle",
    "diamond",
    "ellipse",
    "line",
    "arrow",
    "text",
    "freedraw",
];

fn corpus() -> Vec<(String, String)> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/corpus");
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .map(|entry| entry.expect("directory entry").path())
        .filter(|p| p.extension().is_some_and(|x| x == "excalidraw"))
        .collect();
    paths.sort();
    assert!(
        !paths.is_empty(),
        "no .excalidraw files in {}",
        dir.display()
    );
    paths
        .into_iter()
        .map(|p| {
            let text = std::fs::read_to_string(&p).expect("readable corpus file");
            (p.file_name().unwrap().to_string_lossy().into_owned(), text)
        })
        .collect()
}

#[test]
fn corpus_round_trips() {
    for (name, text) in corpus() {
        let original: Value = serde_json::from_str(&text).expect("corpus file is JSON");
        let file = SceneFile::from_json_str(&text).unwrap_or_else(|e| panic!("{name}: {e}"));
        for element in &file.elements {
            if let Element::Raw(v) = element {
                let kind = v["type"].as_str().unwrap_or_default();
                assert!(
                    !TYPED.contains(&kind),
                    "{name}: {kind} element {} fell back to Raw; its JSON does not fit the struct",
                    v["id"]
                );
            }
        }
        let written: Value = serde_json::from_str(&file.to_json_string()).expect("written JSON");
        assert!(
            semantic_eq(&written, &original),
            "{name} changed on round trip: {}",
            diff(&original, &written, Compare::Exact)
                .unwrap_or_else(|| "difference below 1e-9".into())
        );
    }
}

#[test]
fn every_corpus_element_has_a_placement() {
    for (name, text) in corpus() {
        let file = SceneFile::from_json_str(&text).expect("corpus loads");
        for element in &file.elements {
            assert!(
                element.placement().is_some(),
                "{name}: {} {:?} has no placement",
                element.kind(),
                element.id()
            );
        }
    }
}

#[test]
fn corpus_covers_spec_checklist() {
    let mut seen = BTreeSet::new();
    for (_, text) in corpus() {
        let root: Value = serde_json::from_str(&text).expect("corpus file is JSON");
        if root["files"].as_object().is_some_and(|f| !f.is_empty()) {
            seen.insert("binary files");
        }
        let elements = root["elements"].as_array().cloned().unwrap_or_default();
        let types: HashMap<&str, &str> = elements
            .iter()
            .filter_map(|e| Some((e["id"].as_str()?, e["type"].as_str()?)))
            .collect();
        for e in &elements {
            let kind = e["type"].as_str().unwrap_or_default();
            seen.insert(match kind {
                "image" => "image",
                "frame" => "frame",
                "stickynote" => "sticky note",
                _ => "",
            });
            if kind == "arrow" && (e["startBinding"].is_object() || e["endBinding"].is_object()) {
                seen.insert("arrow binding");
            }
            if kind == "arrow" && e["elbowed"] == true {
                seen.insert("elbow arrow");
            }
            if let Some(container) = e["containerId"].as_str().and_then(|id| types.get(id)) {
                seen.insert(if *container == "arrow" {
                    "arrow label"
                } else {
                    "container text"
                });
            }
            if e["text"]
                .as_str()
                .is_some_and(|t| t.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c)))
            {
                seen.insert("CJK text");
            }
            if e["backgroundColor"]
                .as_str()
                .is_some_and(|c| c != "transparent")
            {
                seen.insert(match e["fillStyle"].as_str() {
                    Some("hachure") => "fill hachure",
                    Some("cross-hatch") => "fill cross-hatch",
                    Some("solid") => "fill solid",
                    Some("zigzag") => "fill zigzag",
                    _ => "",
                });
            }
            seen.insert(match e["strokeStyle"].as_str() {
                Some("dashed") => "dashed stroke",
                Some("dotted") => "dotted stroke",
                _ => "",
            });
            if e["angle"].as_f64().is_some_and(|a| a != 0.0) {
                seen.insert("rotated element");
            }
            if e["groupIds"].as_array().is_some_and(|g| !g.is_empty()) {
                seen.insert("group");
            }
            if e["isDeleted"] == true {
                seen.insert("deleted element");
            }
            if kind == "freedraw" {
                seen.insert(match e["strokeOptions"]["variability"].as_str() {
                    Some("constant") => "freedraw constant",
                    Some("variable") => "freedraw variable",
                    _ => "",
                });
            }
        }
    }
    let required = [
        "arrow binding",
        "arrow label",
        "binary files",
        "CJK text",
        "container text",
        "dashed stroke",
        "deleted element",
        "dotted stroke",
        "elbow arrow",
        "fill cross-hatch",
        "fill hachure",
        "fill solid",
        "fill zigzag",
        "frame",
        "freedraw constant",
        "freedraw variable",
        "group",
        "image",
        "rotated element",
        "sticky note",
    ];
    let missing: Vec<_> = required.iter().filter(|r| !seen.contains(*r)).collect();
    assert!(missing.is_empty(), "corpus lacks: {missing:?}");
}
