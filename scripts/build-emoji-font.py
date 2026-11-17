#!/usr/bin/env python3
"""Builds crates/natizap-ui/assets/fonts/emoji/NatizapEmoji.ttf.

GPUI on Linux only accepts fallback fonts that contain the letter "m", and
only paints glyphs in color for a font whose PostScript name is
"NotoColorEmoji". This takes upstream Noto Color Emoji, maps "m" to its blank
space glyph (never drawn: ASCII always uses the primary font) and renames the
family to "Natizap Emoji", keeping the PostScript name.

Requires fontTools (`pip install fonttools`). Run from the repository root.
"""

import pathlib
import urllib.request

from fontTools.ttLib import TTFont

UPSTREAM_COMMIT = "99701e07d3d1fe1bdbcdb14964547f0fd592966f"
BASE = f"https://raw.githubusercontent.com/googlefonts/noto-emoji/{UPSTREAM_COMMIT}/2D/fonts"
OUT = pathlib.Path("crates/natizap-ui/assets/fonts/emoji")
FAMILY = "Natizap Emoji"


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    source = OUT / "upstream.ttf"
    urllib.request.urlretrieve(f"{BASE}/NotoColorEmoji.ttf", source)
    urllib.request.urlretrieve(f"{BASE}/LICENSE", OUT / "OFL.txt")

    font = TTFont(source)
    for table in font["cmap"].tables:
        # Format 14 holds variation sequences, not plain mappings.
        if table.isUnicode() and table.format != 14:
            table.cmap[ord("m")] = "space"
    for record in font["name"].names:
        if record.nameID in (1, 4, 16):
            record.string = FAMILY
    font.save(OUT / "NatizapEmoji.ttf")
    source.unlink()


if __name__ == "__main__":
    main()
