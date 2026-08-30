#!/usr/bin/env python3
"""Regenerate docs/store/assets icon + feature graphic from src-tauri icons."""
from __future__ import annotations

from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parents[1]
SRC = ROOT / "src-tauri" / "icons" / "256x256.png"
ASSETS = ROOT / "docs" / "store" / "assets"


def load_font(size: int) -> ImageFont.ImageFont:
    for p in (
        "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
        "/usr/share/fonts/truetype/liberation/LiberationSans-Bold.ttf",
        "/usr/share/fonts/truetype/freefont/FreeSansBold.ttf",
    ):
        if Path(p).exists():
            return ImageFont.truetype(p, size)
    return ImageFont.load_default()


def main() -> None:
    ASSETS.mkdir(parents=True, exist_ok=True)
    img = Image.open(SRC).convert("RGBA")
    icon512 = img.resize((512, 512), Image.Resampling.LANCZOS)
    icon_path = ASSETS / "store-icon-512.png"
    icon512.save(icon_path)

    w, h = 1024, 500
    fg = Image.new("RGB", (w, h), "#0e1014")
    for i in range(180):
        alpha = int(28 * (1 - i / 180))
        overlay = Image.new("RGBA", (w, h), (0, 0, 0, 0))
        od = ImageDraw.Draw(overlay)
        od.ellipse((-80 + i, -120 + i // 2, 420 - i, 380 - i // 2), fill=(110, 240, 191, alpha))
        fg = Image.alpha_composite(fg.convert("RGBA"), overlay).convert("RGB")

    icon = icon512.resize((220, 220), Image.Resampling.LANCZOS)
    plate = Image.new("RGBA", (248, 248), (26, 29, 39, 255))
    pd = ImageDraw.Draw(plate)
    pd.rounded_rectangle(
        (0, 0, 247, 247),
        radius=36,
        fill=(26, 29, 39, 255),
        outline=(255, 255, 255, 28),
        width=2,
    )
    plate.paste(icon, (14, 14), icon)
    fg_rgba = fg.convert("RGBA")
    fg_rgba.paste(plate, (64, (h - 248) // 2), plate)
    draw = ImageDraw.Draw(fg_rgba)
    draw.text((360, 160), "ChapterCheck", fill="#f5f7fb", font=load_font(64))
    draw.text(
        (360, 250),
        "Local audiobook & music player for Linux",
        fill="#c2c8d6",
        font=load_font(28),
    )
    draw.text((360, 300), "Resume · Speed · Sleep timer · mpv", fill="#6ef0bf", font=load_font(28))
    out = ASSETS / "feature-graphic-1024x500.png"
    fg_rgba.convert("RGB").save(out, optimize=True)
    print(f"wrote {icon_path}")
    print(f"wrote {out}")


if __name__ == "__main__":
    main()
