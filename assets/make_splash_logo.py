"""Build the splash-screen logo from the app icon.

Run by hand, like make_icon.py:  python assets/make_splash_logo.py

icon.png is a flat black disc with a white "gig" wordmark on transparency. This
gives it depth for the splash screen: an extruded side wall, a glossy face, a
rim light along the top edge and a soft shadow underneath.

To be clear about what this is — a constructed bevel/extrude/gloss effect, not a
rendered 3D model. Every knob is a constant below, so the look can be retuned
without touching the compositing.
"""

import os

from PIL import Image, ImageChops, ImageDraw, ImageFilter

HERE = os.path.dirname(os.path.abspath(__file__))
SOURCE = os.path.join(HERE, "icon.png")
OUTPUT = os.path.join(HERE, "splash_logo.png")

# The sweeping highlight is an image rather than a Slint gradient brush. Both
# renderers this project uses handle gradients, so this is a choice about
# control: the falloff is a raised cosine, shaped here, which makes the edges
# vanish instead of ending at a stop.
SHINE = os.path.join(HERE, "splash_shine.png")
SHINE_W, SHINE_H = 190, 560
SHINE_CORE = 150  # peak alpha at the centre of the streak

# The face is inset in a larger canvas so the extrusion and shadow have room to
# fall without being clipped.
CANVAS = 640
FACE = 480

# How far the disc appears to stand off the surface, in pixels of vertical
# offset. Each step is one stacked copy of the silhouette.
DEPTH = 26

# The side wall darkens with depth, from the top of the extrusion downwards.
WALL_TOP = (86, 86, 92)
WALL_BOTTOM = (10, 10, 14)

# Face gloss: the disc is lifted towards white at the top and pushed towards
# black at the bottom, which is what reads as a curved, polished face.
GLOSS_TOP = 58
GLOSS_BOTTOM = 0

# The rim catches a cool light along its upper edge.
RIM_WIDTH = 5
RIM_COLOR = (228, 238, 255)
RIM_STRENGTH = 200

# A broad soft highlight, upper left, as if lit from that side.
SPEC_STRENGTH = 70

SHADOW_BLUR = 22
SHADOW_OFFSET = 30
SHADOW_ALPHA = 150


def _vertical_ramp(size, top, bottom):
    """A single-channel image ramping from `top` to `bottom`, top to bottom."""
    ramp = Image.new("L", (1, size))
    for y in range(size):
        t = y / max(size - 1, 1)
        ramp.putpixel((0, y), max(0, min(255, int(round(top + (bottom - top) * t)))))
    return ramp.resize((size, size))


def _wall_color_at(t):
    """Side-wall colour a fraction `t` of the way down the extrusion."""
    return tuple(int(round(a + (b - a) * t)) for a, b in zip(WALL_TOP, WALL_BOTTOM))


def build():
    face = Image.open(SOURCE).convert("RGBA").resize((FACE, FACE), Image.LANCZOS)
    silhouette = face.getchannel("A")

    canvas = Image.new("RGBA", (CANVAS, CANVAS), (0, 0, 0, 0))
    left = (CANVAS - FACE) // 2
    top = (CANVAS - FACE - DEPTH) // 2

    # --- shadow, furthest back -------------------------------------------
    shadow = Image.new("RGBA", (CANVAS, CANVAS), (0, 0, 0, 0))
    shadow.paste((0, 0, 0, SHADOW_ALPHA), (left, top + SHADOW_OFFSET), silhouette)
    canvas.alpha_composite(shadow.filter(ImageFilter.GaussianBlur(SHADOW_BLUR)))

    # --- extruded side wall ----------------------------------------------
    # Painted back to front, so the topmost copy — the one nearest the face —
    # is the one left visible.
    for step in range(DEPTH, 0, -1):
        wall = Image.new("RGBA", (CANVAS, CANVAS), (0, 0, 0, 0))
        wall.paste(_wall_color_at(step / DEPTH) + (255,), (left, top + step), silhouette)
        canvas.alpha_composite(wall)

    # --- the face, with its gloss ----------------------------------------
    gloss = _vertical_ramp(FACE, GLOSS_TOP, GLOSS_BOTTOM)
    lit = ImageChops.add(face.convert("RGB"), Image.merge("RGB", (gloss,) * 3))
    face_lit = Image.merge("RGBA", (*lit.split(), silhouette))

    # A broad highlight across the upper left, clipped to the disc.
    spec = Image.new("L", (FACE, FACE), 0)
    ImageDraw.Draw(spec).ellipse(
        (-FACE * 0.25, -FACE * 0.45, FACE * 0.85, FACE * 0.45), fill=SPEC_STRENGTH
    )
    spec = ImageChops.multiply(
        spec.filter(ImageFilter.GaussianBlur(FACE * 0.09)), silhouette
    )
    white = Image.new("L", (FACE, FACE), 255)
    face_lit.alpha_composite(Image.merge("RGBA", (white, white, white, spec)))

    # Rim light: the silhouette minus an eroded copy gives a ring, and keeping
    # the upper part of it puts the light where the specular says it comes from.
    inner = silhouette.filter(ImageFilter.MinFilter(RIM_WIDTH * 2 + 1))
    ring = ImageChops.multiply(
        ImageChops.subtract(silhouette, inner), _vertical_ramp(FACE, RIM_STRENGTH, 0)
    ).filter(ImageFilter.GaussianBlur(1.4))
    face_lit.alpha_composite(
        Image.merge(
            "RGBA", (*(Image.new("L", (FACE, FACE), c) for c in RIM_COLOR), ring)
        )
    )

    canvas.alpha_composite(face_lit, (left, top))
    canvas.save(OUTPUT)
    print(f"Wrote {OUTPUT} ({canvas.size[0]}x{canvas.size[1]})")


def build_shine():
    """A soft vertical streak of light, for sweeping across the disc.

    Alpha falls off as a raised cosine from the centre line outwards, so the
    edges vanish rather than ending — which is the difference between a
    highlight and a white bar.
    """
    import math

    alpha = Image.new("L", (SHINE_W, SHINE_H), 0)
    px = alpha.load()
    mid = (SHINE_W - 1) / 2
    for x in range(SHINE_W):
        t = abs(x - mid) / mid  # 0 at the centre, 1 at the edges
        value = int(round(SHINE_CORE * (0.5 + 0.5 * math.cos(math.pi * t)) ** 1.6))
        for y in range(SHINE_H):
            px[x, y] = value

    white = Image.new("L", (SHINE_W, SHINE_H), 255)
    shine = Image.merge("RGBA", (white, white, white, alpha))
    shine.save(SHINE)
    print(f"Wrote {SHINE} ({SHINE_W}x{SHINE_H})")


if __name__ == "__main__":
    build()
    build_shine()
