#!/usr/bin/env python3
"""Rebuild every raster copy of the app icon from assets/branding/app-icon.svg.

The SVG is the only source. Each size is rendered straight from it by headless
Chrome (never downscaled from a larger bitmap), so the small sizes stay crisp:

  assets/branding/app-icon.png    1024 px
  assets/branding/app-icon.icns   macOS bundle icon (16 to 1024 px, via iconutil)
  assets/branding/app-icon.ico    Windows icon (16 to 256 px)
  assets/branding/portal-logo.png 150 px, developer portal
  site/src/img/app-icon.svg       copy of the SVG for the website logos
  site/src/favicon.ico            16, 32 and 48 px
  site/src/apple-touch-icon.png   180 px
  site/src/icon-192.png, icon-512.png  web manifest

Needs Google Chrome (or Chromium; set CHROME to its binary) and Pillow. The
.icns is only written on macOS, where iconutil exists.
"""
import os
import pathlib
import platform
import shutil
import subprocess
import sys
import tempfile
import time

ROOT = pathlib.Path(__file__).resolve().parents[1]
BRANDING = ROOT / "assets/branding"
SITE = ROOT / "site/src"
SVG = BRANDING / "app-icon.svg"

ICO_SIZES = [16, 24, 32, 48, 64, 128, 256]
FAVICON_SIZES = [16, 32, 48]
# iconutil names: icon_<n>x<n>.png and icon_<n>x<n>@2x.png (2n px).
ICONSET = [16, 32, 128, 256, 512]
SINGLE = {
    BRANDING / "app-icon.png": 1024,
    BRANDING / "portal-logo.png": 150,
    SITE / "apple-touch-icon.png": 180,
    SITE / "icon-192.png": 192,
    SITE / "icon-512.png": 512,
}
CHROME_CANDIDATES = [
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    "/Applications/Chromium.app/Contents/MacOS/Chromium",
    "google-chrome",
    "google-chrome-stable",
    "chromium",
    "chromium-browser",
    r"C:\Program Files\Google\Chrome\Application\chrome.exe",
]


def find_chrome():
    for candidate in [os.environ.get("CHROME")] + CHROME_CANDIDATES:
        if not candidate:
            continue
        found = shutil.which(candidate) or (candidate if pathlib.Path(candidate).is_file() else None)
        if found:
            return found
    raise SystemExit("Google Chrome or Chromium is required to render the icons: install it or set CHROME to its binary")


def render(chrome, work, size):
    """Screenshot the SVG laid out at exactly size x size CSS px, 1 px per CSS px."""
    page = work / f"icon-{size}.html"
    page.write_text(
        "<!doctype html><html><head><style>html,body{margin:0;padding:0;background:transparent}"
        f"img{{display:block;width:{size}px;height:{size}px}}</style></head>"
        f'<body><img src="{SVG.as_uri()}"></body></html>'
    )
    shot = work / f"icon-{size}.png"
    # Headless Chrome may keep running after it writes the screenshot, so wait
    # for the file and stop the browser ourselves.
    proc = subprocess.Popen(
        [chrome, "--headless=new", f"--user-data-dir={work / 'profile'}", "--disable-gpu",
         "--no-first-run", "--no-default-browser-check", "--hide-scrollbars",
         "--allow-file-access-from-files", "--default-background-color=00000000",
         "--force-device-scale-factor=1", f"--window-size={size},{size}",
         f"--screenshot={shot}", page.as_uri()],
        stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True,
    )
    deadline = time.monotonic() + 60
    try:
        while proc.poll() is None and not (shot.exists() and shot.stat().st_size):
            if time.monotonic() > deadline:
                raise SystemExit(f"Chrome did not render the {size} px icon within 60 s")
            time.sleep(0.2)
        time.sleep(0.5)  # let Chrome finish flushing the file
    finally:
        if proc.poll() is None:
            proc.terminate()
            try:
                proc.wait(10)
            except subprocess.TimeoutExpired:
                proc.kill()
                proc.wait()
    if not shot.exists():
        raise SystemExit(f"Chrome wrote no screenshot for {size} px:\n{proc.stdout.read()}")
    from PIL import Image
    with Image.open(shot) as image:
        image = image.convert("RGBA")
    if image.size != (size, size):
        raise SystemExit(f"Chrome rendered {image.size} instead of {size} px")
    return image


def save_ico(path, images):
    sizes = sorted(images)
    largest = images[sizes[-1]]
    # Pillow takes a frame from append_images whenever its size is requested,
    # so every entry is the direct render rather than a resample.
    largest.save(path, format="ICO", sizes=[(s, s) for s in sizes],
                 append_images=[images[s] for s in sizes[:-1]])


def main():
    if not SVG.is_file():
        raise SystemExit(f"Missing {SVG}")
    try:
        import PIL  # noqa: F401
    except ImportError:
        raise SystemExit("Pillow is required: python3 -m pip install Pillow")
    chrome = find_chrome()
    sizes = set(SINGLE.values()) | set(ICO_SIZES) | set(FAVICON_SIZES)
    sizes |= set(ICONSET) | {2 * s for s in ICONSET}
    with tempfile.TemporaryDirectory(prefix="yoto-icons-") as tmp:
        work = pathlib.Path(tmp)
        images = {}
        for size in sorted(sizes):
            print(f"render {size} px")
            images[size] = render(chrome, work, size)

        for path, size in SINGLE.items():
            images[size].save(path, optimize=True)
        save_ico(BRANDING / "app-icon.ico", {s: images[s] for s in ICO_SIZES})
        save_ico(SITE / "favicon.ico", {s: images[s] for s in FAVICON_SIZES})
        shutil.copyfile(SVG, SITE / "img/app-icon.svg")

        if platform.system() == "Darwin":
            iconset = work / "app-icon.iconset"
            iconset.mkdir()
            for size in ICONSET:
                images[size].save(iconset / f"icon_{size}x{size}.png")
                images[2 * size].save(iconset / f"icon_{size}x{size}@2x.png")
            subprocess.run(["iconutil", "-c", "icns", str(iconset), "-o", str(BRANDING / "app-icon.icns")], check=True)
        else:
            print("Skipped app-icon.icns: iconutil only exists on macOS", file=sys.stderr)
    print("Icons rebuilt from", SVG.relative_to(ROOT))


if __name__ == "__main__":
    main()
