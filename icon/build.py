#!/usr/bin/env python3
"""Draw the STO-CLARE application icon.

This script is the source of the icon — there is no vector master, because the
glass look is gradients, blurs and soft shadows, which are read off these
numbers rather than off a drawing. It writes the two files that ship:

    icon.png   512x512 RGBA — window icon, desktop entry, macOS bundle
    icon.ico   16..256       — the exe's own icon and the Windows installer

Both are committed, so an ordinary build needs neither this script nor Pillow.
Re-run it after changing anything here:

    python3 icon/build.py

The mark is `delta-mask.png`, a grayscale silhouette; everything else is drawn.
Work happens at 1024px and is scaled down, so edges and blurs stay smooth at
every size that ships.
"""

from pathlib import Path

from PIL import Image, ImageChops, ImageDraw, ImageFilter, ImageFont

HERE = Path(__file__).resolve().parent
FONT = HERE.parent / "assets" / "fonts" / "Ubuntu-Bold.ttf"
MASK = HERE / "delta-mask.png"

SIZE = 1024
RADIUS = int(SIZE * 0.22)
ICO_SIZES = [16, 24, 32, 48, 64, 128, 256]

BLUE = (54, 56, 189)  # the blue of the sto-warp icon's corner
RED = (232, 42, 48)

# The mark, and the lettering across it.
DELTA_HEIGHT = 0.76  # of the tile
DELTA_CENTRE_Y = 0.45
TEXT = "CLA"
TEXT_MARGIN = 0.07  # of the tile, left and right
TEXT_CENTRE_Y = 0.50


def rounded(radius):
    mask = Image.new("L", (SIZE, SIZE), 0)
    ImageDraw.Draw(mask).rounded_rectangle(
        [0, 0, SIZE - 1, SIZE - 1], radius=radius, fill=255
    )
    return mask


def vertical_gradient(top, bottom):
    column = Image.new("RGB", (1, SIZE))
    for y in range(SIZE):
        t = y / (SIZE - 1)
        column.putpixel(
            (0, y), tuple(round(top[i] + (bottom[i] - top[i]) * t) for i in range(3))
        )
    return column.resize((SIZE, SIZE), Image.BICUBIC)


def mix(colour, other, t):
    return tuple(round(colour[i] + (other[i] - colour[i]) * t) for i in range(3))


def mark():
    """The delta, centred, as a mask over the whole tile."""
    delta = Image.open(MASK).convert("L")
    height = int(SIZE * DELTA_HEIGHT)
    width = round(delta.width * height / delta.height)
    canvas = Image.new("L", (SIZE, SIZE), 0)
    canvas.paste(
        delta.resize((width, height), Image.LANCZOS),
        (SIZE // 2 - width // 2, int(SIZE * DELTA_CENTRE_Y) - height // 2),
    )
    return canvas


def specular():
    """Light falling from the top left — what makes the tile read as glass."""
    layer = Image.new("L", (SIZE, SIZE), 0)
    ImageDraw.Draw(layer).ellipse(
        [-SIZE * 0.45, -SIZE * 0.75, SIZE * 0.95, SIZE * 0.42], fill=160
    )
    return layer.filter(ImageFilter.GaussianBlur(SIZE * 0.09))


def edge_light(width=5):
    """The lit rim of a glass slab."""
    outer = rounded(RADIUS)
    inner = Image.new("L", (SIZE, SIZE), 0)
    ImageDraw.Draw(inner).rounded_rectangle(
        [width, width, SIZE - 1 - width, SIZE - 1 - width],
        radius=RADIUS - width,
        fill=255,
    )
    return ImageChops.subtract(outer, inner)


def font_for_width(target_width):
    """Largest size at which the lettering still keeps its margin. Sized by the
    tile rather than by the mark, so it spans the icon."""
    measure = ImageDraw.Draw(Image.new("L", (10, 10)))
    size = 10
    while True:
        candidate = ImageFont.truetype(str(FONT), size + 4)
        left, _, right, _ = measure.textbbox((0, 0), TEXT, font=candidate)
        if right - left > target_width:
            return ImageFont.truetype(str(FONT), size)
        size += 4


def draw():
    tile = vertical_gradient(
        mix(BLUE, (255, 255, 255), 0.12), mix(BLUE, (0, 0, 0), 0.62)
    ).convert("RGBA")
    delta = mark()

    # The mark sits above the tile, so it casts a shadow onto it.
    shadow = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
    shadow.paste(
        Image.new("RGBA", (SIZE, SIZE), (0, 0, 25, 170)), (0, int(SIZE * 0.02)), delta
    )
    tile.alpha_composite(shadow.filter(ImageFilter.GaussianBlur(SIZE * 0.028)))

    # The mark itself: white glass, cooler towards the bottom, lit from above.
    glass = vertical_gradient((255, 255, 255), (190, 196, 255)).convert("RGBA")
    glass.putalpha(215)
    tile.paste(glass, (0, 0), delta)
    tile.paste(
        Image.new("RGBA", (SIZE, SIZE), (255, 255, 255, 255)),
        (0, 0),
        ImageChops.multiply(delta, specular()),
    )

    # The lettering runs the width of the tile, so it crosses both the white
    # glass and the blue. Its own shadow is what keeps it legible on both.
    font = font_for_width(SIZE * (1 - 2 * TEXT_MARGIN))
    layer = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
    pen = ImageDraw.Draw(layer)
    left, top, right, bottom = pen.textbbox((0, 0), TEXT, font=font)
    x = SIZE / 2 - (right - left) / 2 - left
    y = SIZE * TEXT_CENTRE_Y - (bottom - top) / 2 - top
    cast = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
    ImageDraw.Draw(cast).text(
        (x, y + SIZE * 0.012), TEXT, font=font, fill=(0, 0, 20, 190)
    )
    tile.alpha_composite(cast.filter(ImageFilter.GaussianBlur(SIZE * 0.012)))
    pen.text((x, y), TEXT, font=font, fill=RED + (255,))
    tile.alpha_composite(layer)

    # Thickness: dark inside the bottom edge, bright along the rim.
    ring = Image.new("L", (SIZE, SIZE), 0)
    ImageDraw.Draw(ring).rounded_rectangle(
        [0, 0, SIZE - 1, SIZE - 1], radius=RADIUS, outline=255, width=18
    )
    tile.paste(
        Image.new("RGBA", (SIZE, SIZE), (0, 0, 25, 255)),
        (0, 0),
        ring.filter(ImageFilter.GaussianBlur(18)).point(lambda v: int(v * 80 / 255)),
    )
    tile.paste(
        Image.new("RGBA", (SIZE, SIZE), (255, 255, 255, 255)),
        (0, 0),
        edge_light().point(lambda v: int(v * 0.5)),
    )

    out = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
    out.paste(tile, (0, 0), rounded(RADIUS))
    return out


def main():
    master = draw()

    png = master.resize((512, 512), Image.LANCZOS)
    png.save(HERE / "icon.png")

    frames = [master.resize((s, s), Image.LANCZOS) for s in ICO_SIZES]
    frames[-1].save(
        HERE / "icon.ico",
        format="ICO",
        sizes=[(s, s) for s in ICO_SIZES],
        append_images=frames[:-1],
    )
    print(f"wrote {HERE / 'icon.png'} and {HERE / 'icon.ico'}")


if __name__ == "__main__":
    main()
