#!/usr/bin/env python3
"""Generate the 1024×1024 Snap 24 app icon (App Store requires one 1024 icon).

    python3 make_icon.py

Uses the same palette/type as the game: warm near-black, ivory card, brass "24".
"""

from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter, ImageFont

SIZE = 1024
BG = (15, 12, 10)
GLOW = (34, 25, 18)
IVORY = (243, 236, 224)
BRASS = (201, 162, 74)
INK = (26, 20, 15)

HERE = Path(__file__).parent
FRAUNCES = HERE.parent / "assets" / "fonts" / "Fraunces-Black.ttf"


def rounded_card(size, radius):
    card = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    ImageDraw.Draw(card).rounded_rectangle((0, 0, size - 1, size - 1), radius, fill=IVORY)
    return card


def main() -> None:
    img = Image.new("RGB", (SIZE, SIZE), BG)

    # Warm radial glow from the top, like the in-game vignette.
    glow = Image.new("L", (SIZE, SIZE), 0)
    gd = ImageDraw.Draw(glow)
    for r in range(SIZE // 2, 0, -8):
        shade = int(70 * (1 - r / (SIZE / 2)) ** 2)
        gd.ellipse(
            (SIZE // 2 - r, SIZE // 3 - r, SIZE // 2 + r, SIZE // 3 + r),
            fill=shade,
        )
    glow = glow.filter(ImageFilter.GaussianBlur(40))
    img = Image.composite(Image.new("RGB", (SIZE, SIZE), GLOW), img, glow)

    # The card, slightly rotated, with a soft shadow.
    card = rounded_card(560, 64).rotate(-7, expand=True, resample=Image.BICUBIC)
    shadow = Image.new("RGBA", img.size, (0, 0, 0, 0))
    shadow.paste(card, ((SIZE - card.width) // 2 + 6, (SIZE - card.height) // 2 + 26), card)
    shadow = shadow.filter(ImageFilter.GaussianBlur(28))
    img = Image.alpha_composite(img.convert("RGBA"), shadow).convert("RGB")

    card_pos = ((SIZE - card.width) // 2, (SIZE - card.height) // 2)
    img.paste(card, card_pos, card)

    # "24" in Fraunces, centred on the card.
    draw = ImageDraw.Draw(img)
    font = ImageFont.truetype(str(FRAUNCES), 340)
    text = "24"
    box = draw.textbbox((0, 0), text, font=font)
    draw.text(
        (SIZE // 2 - (box[2] - box[0]) / 2 - box[0],
         SIZE // 2 - (box[3] - box[1]) / 2 - box[1] - 30),
        text,
        font=font,
        fill=BRASS,
    )

    # A small suit pip, bottom-centre of the card.
    pip = ImageFont.truetype(str(HERE.parent / "assets" / "fonts" / "DejaVuSans.ttf"), 130)
    pbox = draw.textbbox((0, 0), "♠", font=pip)
    draw.text(
        (SIZE // 2 - (pbox[2] - pbox[0]) / 2 - pbox[0], SIZE // 2 + 170),
        "♠",
        font=pip,
        fill=INK,
    )

    out = HERE / "Assets.xcassets" / "AppIcon.appiconset" / "AppIcon-1024.png"
    out.parent.mkdir(parents=True, exist_ok=True)
    img.save(out)
    print(f"wrote {out}")


if __name__ == "__main__":
    main()
