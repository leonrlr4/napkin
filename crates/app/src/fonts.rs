//! Registers napkin's bundled fonts, and the system CJK font when one is installed, as egui
//! font families: `text_edit`'s overlay needs napkin's own fonts to preview a text element's
//! `fontFamily`, and needs a CJK font (for typed Chinese and IME candidates) since egui's own
//! default fonts only cover Latin and Cyrillic.

use eframe::egui;

use crate::render::text;

/// Family names `fontdb` may report the system CJK font under, tried in this order: the same
/// discovery mechanism `render::text::font_system` relies on for shaping, whichever of
/// Traditional, Simplified or Japanese Noto Sans CJK is actually installed.
const CJK_FAMILY_NAMES: [&str; 3] = ["Noto Sans CJK TC", "Noto Sans CJK SC", "Noto Sans CJK JP"];

/// The egui font name the system CJK font (when found) is registered under.
const CJK_FONT_NAME: &str = "napkin-cjk";

/// The three bundled font names, matching `render::text::bundled_family`'s return values.
const BUNDLED_FAMILIES: [&str; 3] = ["napkin-hand", "napkin-sans", "napkin-code"];

/// Finds the system CJK font through `fontdb`: its raw bytes and face index (a `.ttc` file
/// bundles several faces together, so the index says which one to use). `None` when no
/// `CJK_FAMILY_NAMES` entry is installed; the caller then simply registers no CJK fallback.
/// `db` is expected to already have the system's fonts loaded (`cosmic_text::FontSystem::new`
/// does this itself); scanning again here just to look one up would cost a second system font
/// scan for no benefit.
fn find_cjk_font(db: &glyphon::fontdb::Database) -> Option<(Vec<u8>, u32)> {
    for name in CJK_FAMILY_NAMES {
        let id = db
            .faces()
            .find(|face| face.families.iter().any(|(family, _)| family == name))
            .map(|face| face.id);
        if let Some(id) = id
            && let Some(result) = db.with_face_data(id, |data, index| (data.to_vec(), index))
        {
            return Some(result);
        }
    }
    None
}

/// [`text::bundled_family`] as an egui family: napkin's three bundled fonts are registered
/// under their own names by [`install`].
pub fn egui_family(font_family: f64) -> egui::FontFamily {
    egui::FontFamily::Name(text::bundled_family(font_family).into())
}

/// Registers napkin-hand, napkin-sans and napkin-code as egui font families (named after
/// themselves), each falling back to the system CJK font when one is found, and adds that CJK
/// font as a fallback of egui's proportional family too, so IME candidates and typed Chinese
/// show both in a `TextEdit` using a bundled family and in egui's own built-in UI text. `db` is
/// an already-populated font database (see [`find_cjk_font`]) to look the CJK font up in.
pub fn install(ctx: &egui::Context, db: &glyphon::fontdb::Database) {
    let mut fonts = egui::FontDefinitions::default();

    fonts.font_data.insert(
        "napkin-hand".to_owned(),
        egui::FontData::from_owned(
            include_bytes!("../../../assets/fonts/napkin-hand.ttf").to_vec(),
        )
        .into(),
    );
    fonts.font_data.insert(
        "napkin-sans".to_owned(),
        egui::FontData::from_owned(
            include_bytes!("../../../assets/fonts/napkin-sans.ttf").to_vec(),
        )
        .into(),
    );
    fonts.font_data.insert(
        "napkin-code".to_owned(),
        egui::FontData::from_owned(
            include_bytes!("../../../assets/fonts/napkin-code.ttf").to_vec(),
        )
        .into(),
    );

    let cjk = find_cjk_font(db);
    if let Some((data, index)) = &cjk {
        let mut font_data = egui::FontData::from_owned(data.clone());
        font_data.index = *index;
        fonts
            .font_data
            .insert(CJK_FONT_NAME.to_owned(), font_data.into());
    }

    for name in BUNDLED_FAMILIES {
        let mut family = vec![name.to_owned()];
        if cjk.is_some() {
            family.push(CJK_FONT_NAME.to_owned());
        }
        fonts
            .families
            .insert(egui::FontFamily::Name(name.into()), family);
    }

    if cjk.is_some() {
        fonts
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default()
            .push(CJK_FONT_NAME.to_owned());
    }

    ctx.set_fonts(fonts);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_font_families_map_to_named_egui_families() {
        assert_eq!(
            egui_family(5.0),
            egui::FontFamily::Name("napkin-hand".into())
        );
        assert_eq!(
            egui_family(42.0),
            egui::FontFamily::Name("napkin-hand".into())
        );
    }

    #[test]
    fn install_registers_the_three_bundled_families() {
        let ctx = egui::Context::default();
        // Empty rather than a real system font scan: this only checks the bundled families
        // themselves, not CJK discovery, and a real scan would make every run of this test pay
        // for one.
        install(&ctx, &glyphon::fontdb::Database::new());
        // Font definitions only take effect at the start of the next pass. `FullOutput` panics
        // on drop if its `textures_delta` (here, just the font atlas) is not explicitly cleared.
        let mut output = ctx.run_ui(Default::default(), |_| {});
        output.textures_delta.clear();
        let families = ctx.fonts(|fonts| fonts.families());
        for name in BUNDLED_FAMILIES {
            assert!(
                families.contains(&egui::FontFamily::Name(name.into())),
                "{name} missing from {families:?}"
            );
        }
    }
}
