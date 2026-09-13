#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.12"
# dependencies = ["fonttools[woff]==4.65.0"]
# ///
"""Build napkin's bundled fonts from the woff2 subsets in a pinned Excalidraw commit.

Excalidraw ships each font split into unicode-range woff2 subsets and no full upstream
font is published, so this merges the subsets into one TTF per font.

The merged fonts are renamed napkin-hand / napkin-sans / napkin-code. Excalifont's name
table says "Excalifont is a trademark of Excalidraw" and OFL grants no trademark rights,
so a modified build must not present itself under that name; the other two follow the
same rule for uniformity. `.excalidraw` files store a numeric fontFamily, not a name, so
renaming does not affect file compatibility.

Run from anywhere: `tools/fonts/build_fonts.py`. Writes assets/fonts/ and exits non-zero
if any codepoint of any subset is missing from, or maps to a different glyph in, the
merged font.
"""

import io
import json
import sys
import tempfile
import urllib.request
from pathlib import Path

from fontTools.merge import Merger
from fontTools.ttLib import TTFont

EXCALIDRAW_COMMIT = "afa3a653fc5d2b742adcbd5a6063187b056d2419"
FONTS_PATH = "packages/excalidraw/fonts"
OUT_DIR = Path(__file__).resolve().parents[2] / "assets" / "fonts"

# (directory under FONTS_PATH, napkin family name, license of the source font)
FONTS = [
    ("Excalifont", "napkin-hand", "OFL-1.1"),
    ("Nunito", "napkin-sans", "OFL-1.1"),
    ("ComicShanns", "napkin-code", "MIT"),
]

# Tables the merger cannot combine and napkin does not need. Nunito's STAT only describes
# its variable-font design axes, which a static merged instance no longer has.
DROPPED_TABLES = ("STAT",)


def fetch(url: str) -> bytes:
    request = urllib.request.Request(url, headers={"User-Agent": "napkin-build-fonts"})
    with urllib.request.urlopen(request, timeout=60) as response:
        return response.read()


def raw_url(path: str) -> str:
    return f"https://raw.githubusercontent.com/excalidraw/excalidraw/{EXCALIDRAW_COMMIT}/{path}"


def list_subsets(directory: str) -> list[str]:
    api = (
        "https://api.github.com/repos/excalidraw/excalidraw/contents/"
        f"{FONTS_PATH}/{directory}?ref={EXCALIDRAW_COMMIT}"
    )
    entries = json.loads(fetch(api))
    return sorted(e["path"] for e in entries if e["name"].endswith(".woff2"))


def ofl_text() -> str:
    """The full OFL 1.1 text, taken from the header comment of Excalifont's index.ts."""
    source = fetch(raw_url(f"{FONTS_PATH}/Excalifont/index.ts")).decode()
    start = source.index("license: ") + len("license: ")
    end = source.index("\nlicenseURL:", start)
    return source[start:end].strip() + "\n"


def decompress(woff2_path: str, workdir: Path) -> Path:
    font = TTFont(io.BytesIO(fetch(raw_url(woff2_path))))
    font.flavor = None
    for tag in DROPPED_TABLES:
        if tag in font:
            del font[tag]
    out = workdir / (Path(woff2_path).stem + ".ttf")
    font.save(out)
    return out


def rename(font: TTFont, family: str) -> None:
    postscript = f"{family}-Regular"
    names = {
        1: family,
        2: "Regular",
        3: f"{postscript};napkin",
        4: f"{family} Regular",
        6: postscript,
        16: family,
        17: "Regular",
    }
    table = font["name"]
    for name_id in (1, 2, 3, 4, 6, 16, 17, 21, 22, 25):
        table.removeNames(nameID=name_id)
    for name_id, value in names.items():
        table.setName(value, name_id, 3, 1, 0x409)  # Windows, Unicode BMP, en-US
        table.setName(value, name_id, 1, 0, 0)  # Macintosh, Roman, English


def glyph_signature(font: TTFont, glyph: str) -> tuple:
    outline = font["glyf"][glyph]
    outline.recalcBounds(font["glyf"])
    bounds = tuple(getattr(outline, k, 0) for k in ("xMin", "yMin", "xMax", "yMax"))
    return font["hmtx"][glyph][0], bounds


def mismatches(subsets: list[TTFont], merged: TTFont) -> list[str]:
    merged_cmap = merged.getBestCmap()
    errors = []
    for subset in subsets:
        for codepoint, glyph in subset.getBestCmap().items():
            if codepoint not in merged_cmap:
                errors.append(f"U+{codepoint:04X} missing from merged font")
                continue
            expected = glyph_signature(subset, glyph)
            actual = glyph_signature(merged, merged_cmap[codepoint])
            if expected != actual:
                errors.append(f"U+{codepoint:04X} glyph differs: {expected} != {actual}")
    return errors


def license_text(directory: str, family: str, kind: str, source: TTFont, ofl: str) -> str:
    copyright_notice = source["name"].getDebugName(0) or ""
    provenance = (
        f"{family} is built by napkin's tools/fonts/build_fonts.py from the {directory}\n"
        f"woff2 subsets in Excalidraw commit {EXCALIDRAW_COMMIT}\n"
        f"({FONTS_PATH}/{directory}). Modifications: subsets merged into one font,\n"
        f"renamed to {family}.\n\n"
    )
    if kind == "OFL-1.1":
        return f"{provenance}{copyright_notice}\n\n{ofl}"
    # Comic Shanns carries its complete MIT license in the copyright name record.
    assert "Permission is hereby granted" in copyright_notice, f"{directory}: MIT text not found"
    return f"{provenance}{copyright_notice}\n"


def build(directory: str, family: str, kind: str, ofl: str) -> list[str]:
    with tempfile.TemporaryDirectory() as tmp:
        paths = [decompress(p, Path(tmp)) for p in list_subsets(directory)]
        subsets = [TTFont(p) for p in paths]
        merged = Merger().merge([str(p) for p in paths])
        rename(merged, family)
        out = OUT_DIR / f"{family}.ttf"
        merged.save(out)
        written = TTFont(out)
        errors = mismatches(subsets, written)
        (OUT_DIR / f"{family}.LICENSE.txt").write_text(
            license_text(directory, family, kind, subsets[0], ofl)
        )
        print(
            f"{family}: {len(paths)} subsets -> {out.name}, "
            f"{len(written.getBestCmap())} codepoints, {len(errors)} mismatches"
        )
        return errors


def main() -> int:
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    ofl = ofl_text()
    failed = False
    for directory, family, kind in FONTS:
        errors = build(directory, family, kind, ofl)
        for error in errors[:20]:
            print(f"  {error}")
        failed = failed or bool(errors)
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
