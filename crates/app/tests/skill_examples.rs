//! Every ```json block in skills/napkin/SKILL.md is a batch that must apply cleanly to an
//! empty scene, so the skill cannot drift from what `napkin apply` accepts.

use scene::editor::Editor;
use scene::sample::{self, CharWidthMeasure};

#[test]
fn skill_batches_apply_to_an_empty_scene() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../skills/napkin/SKILL.md");
    let text = std::fs::read_to_string(path).expect("SKILL.md");
    let blocks: Vec<&str> = text
        .split("```json\n")
        .skip(1)
        .map(|rest| rest.split("```").next().expect("closed block"))
        .collect();
    assert!(blocks.len() >= 3, "the skill shows at least three batches");
    for block in blocks {
        let batch: serde_json::Value =
            serde_json::from_str(block).unwrap_or_else(|e| panic!("{e}\n{block}"));
        let mut editor = Editor::new(sample::file(vec![]), scene::env::SystemEnv);
        if let Err(errors) = editor.apply_batch(&batch, &mut CharWidthMeasure) {
            panic!("{errors:?}\n{block}");
        }
    }
}
