#!/usr/bin/env python3
"""Regenerate responsive WebP guide images from the checked-in PNG originals.

Requires Pillow in the developer's Python environment. Generated files are
checked in, so the Eleventy build does not need Python or an image service.
"""
from pathlib import Path
from PIL import Image

images = Path(__file__).resolve().parents[1] / "src/docs/images"
original_bytes = generated_bytes = 0
for source in sorted(images.glob("*.png")):
    with Image.open(source) as original:
        original_bytes += source.stat().st_size
        for width in (480, original.width):
            image = original.convert("RGB")
            if width < original.width:
                image = image.resize((width, round(original.height * width / original.width)), Image.Resampling.LANCZOS)
            output = source.with_name(f"{source.stem}-{width}.webp")
            image.save(output, "WEBP", lossless=True, method=6)
            generated_bytes += output.stat().st_size
            print(f"{output.name}: {output.stat().st_size:,} bytes")
print(f"Original PNG total: {original_bytes:,}; both WebP sizes: {generated_bytes:,} bytes")
