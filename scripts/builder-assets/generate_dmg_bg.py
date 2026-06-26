#!/usr/bin/env python3
"""Generate a very simple DMG background for Mini Pi."""

from PIL import Image, ImageDraw, ImageFont

WIDTH, HEIGHT = 560, 400

# Almost-white background
img = Image.new("RGB", (WIDTH, HEIGHT), "#FAFBFC")
draw = ImageDraw.Draw(img)

# Fonts
try:
    title_font = ImageFont.truetype("/System/Library/Fonts/Helvetica.ttc", 40)
    body_font = ImageFont.truetype("/System/Library/Fonts/Helvetica.ttc", 17)
except OSError:
    title_font = ImageFont.load_default()
    body_font = title_font

# Title
TITLE = "Mini Pi"
bbox = draw.textbbox((0, 0), TITLE, font=title_font)
title_w = bbox[2] - bbox[0]
draw.text(((WIDTH - title_w) // 2, 50), TITLE, font=title_font, fill="#2C3E50")

# Simple arrow between the two icon positions (25% / 75% of width)
left_x, right_x = 140, 420
arrow_y = 200
arrow_start = left_x + 55
arrow_end = right_x - 55
draw.line([(arrow_start, arrow_y), (arrow_end, arrow_y)], fill="#95A5A6", width=2)
head = 7
draw.polygon([
    (arrow_end, arrow_y),
    (arrow_end - head, arrow_y - head),
    (arrow_end - head, arrow_y + head),
], fill="#95A5A6")

# Bottom instruction
instruction = "Drag the app to Applications"
bbox = draw.textbbox((0, 0), instruction, font=body_font)
instr_w = bbox[2] - bbox[0]
draw.text(((WIDTH - instr_w) // 2, HEIGHT - 55), instruction, font=body_font, fill="#7F8C8D")

img.save("scripts/builder-assets/dmg-background.png", "PNG")
print("Generated scripts/builder-assets/dmg-background.png")
