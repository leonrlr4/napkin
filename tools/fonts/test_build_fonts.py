"""Guards the merge check in build_fonts.py: it must reject a bad merge, not just pass.

Runs against the committed assets/fonts, so it needs no network:
    uv run --with 'fonttools[woff]==4.65.0' --with pytest pytest tools/fonts
"""

import importlib.util
from pathlib import Path

from fontTools.ttLib import TTFont

_spec = importlib.util.spec_from_file_location(
    "build_fonts", Path(__file__).resolve().parent / "build_fonts.py"
)
build_fonts = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(build_fonts)

# napkin-code is monospaced, so every glyph has the same advance width: a wrong-glyph
# mapping is only detectable through the outline bounds, which is what the test needs.
FONT = build_fonts.OUT_DIR / "napkin-code.ttf"


def unicode_cmaps(font: TTFont) -> list[dict[int, str]]:
    return [table.cmap for table in font["cmap"].tables if table.isUnicode()]


def test_identical_fonts_have_no_mismatches():
    assert build_fonts.mismatches([TTFont(FONT)], TTFont(FONT)) == []


def test_missing_codepoint_is_reported():
    reference, broken = TTFont(FONT), TTFont(FONT)
    for cmap in unicode_cmaps(broken):
        cmap.pop(ord("A"), None)

    assert "U+0041 missing from merged font" in build_fonts.mismatches([reference], broken)


def test_codepoint_mapped_to_wrong_glyph_is_reported():
    reference, broken = TTFont(FONT), TTFont(FONT)
    wrong_glyph = reference.getBestCmap()[ord("W")]
    for cmap in unicode_cmaps(broken):
        if ord("B") in cmap:
            cmap[ord("B")] = wrong_glyph

    errors = build_fonts.mismatches([reference], broken)

    assert any(error.startswith("U+0042 glyph differs") for error in errors)


# spec §6.3 requires the merged fonts to shed Excalifont's name so a modified build
# does not present itself as the trademarked original (see build_fonts.py module
# docstring). These guard that rename against regressing back to the upstream name.
COMMITTED_FONTS = {
    "napkin-hand": build_fonts.OUT_DIR / "napkin-hand.ttf",
    "napkin-sans": build_fonts.OUT_DIR / "napkin-sans.ttf",
    "napkin-code": build_fonts.OUT_DIR / "napkin-code.ttf",
}


def test_committed_fonts_have_no_rename_violations():
    for family, path in COMMITTED_FONTS.items():
        assert build_fonts.rename_violations(TTFont(path), family) == []


def test_reverted_name_is_reported():
    font = TTFont(FONT)
    font["name"].setName("Excalifont", 1, 3, 1, 0x409)

    violations = build_fonts.rename_violations(font, "napkin-code")

    assert any("Excalifont" in v for v in violations)
