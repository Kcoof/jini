# Composite a transparent UI capture from CDP onto a clean gradient backdrop
# Usage: python scripts/make-hero.py <ui.png> <out.png> [width height scale]
import sys
from PIL import Image, ImageDraw, ImageFilter

ui_path, out_path = sys.argv[1], sys.argv[2]
W = int(sys.argv[3]) if len(sys.argv) > 3 else 1400
H = int(sys.argv[4]) if len(sys.argv) > 4 else 720
SCALE = float(sys.argv[5]) if len(sys.argv) > 5 else 1.25

ui = Image.open(ui_path).convert("RGBA")
if SCALE != 1.0:
    ui = ui.resize((int(ui.width * SCALE), int(ui.height * SCALE)), Image.LANCZOS)

# Backdrop: vertical gradient #0b1220 -> #1c2a4a with a soft indigo glow behind the UI
top = (11, 18, 32)
bottom = (26, 40, 72)
bg = Image.new("RGB", (W, H))
px = bg.load()
for y in range(H):
    t = y / (H - 1)
    r = int(top[0] + (bottom[0] - top[0]) * t)
    g = int(top[1] + (bottom[1] - top[1]) * t)
    b = int(top[2] + (bottom[2] - top[2]) * t)
    for x in range(W):
        px[x, y] = (r, g, b)

glow = Image.new("L", (W, H), 0)
gd = ImageDraw.Draw(glow)
cx, cy = W // 2, H // 2
gr = int(max(ui.width, ui.height) * 0.95)
gd.ellipse([cx - gr, cy - gr, cx + gr, cy + gr], fill=70)
glow = glow.filter(ImageFilter.GaussianBlur(120))
indigo = Image.new("RGB", (W, H), (67, 82, 158))
bg = Image.composite(indigo, bg, glow.point(lambda v: v))

# Soft drop shadow from the UI alpha
shadow = Image.new("RGBA", (W, H), (0, 0, 0, 0))
sh = Image.new("L", (W, H), 0)
alpha = ui.getchannel("A")
sh.paste(alpha, ((W - ui.width) // 2, (H - ui.height) // 2 + 18))
sh = sh.filter(ImageFilter.GaussianBlur(22)).point(lambda v: int(v * 0.55))
shadow.putalpha(sh)
black = Image.new("RGBA", (W, H), (0, 0, 0, 255))
shadow = Image.composite(black, shadow, sh.point(lambda v: 255 if v > 0 else 0))
shadow.putalpha(sh)

out = bg.convert("RGBA")
out.alpha_composite(shadow)
out.alpha_composite(ui, ((W - ui.width) // 2, (H - ui.height) // 2))
out.convert("RGB").save(out_path, "PNG")
print(f"SAVED {out_path} {W}x{H}")
