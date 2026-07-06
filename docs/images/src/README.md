# README image sources

The HTML files in this folder are the editable sources for the PNGs in `docs/images/`
(used by the top-level `README.md`). Edit the HTML, then regenerate with headless Chrome:

```bash
cd docs/images/src
CHROME="/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"

"$CHROME" --headless=new --disable-gpu --hide-scrollbars --force-device-scale-factor=2 \
  --window-size=880,360 --screenshot=../hero.png "file://$PWD/hero.html"

"$CHROME" --headless=new --disable-gpu --hide-scrollbars --force-device-scale-factor=2 \
  --window-size=880,584 --screenshot=../how-it-works.png "file://$PWD/how-it-works.html"

"$CHROME" --headless=new --disable-gpu --hide-scrollbars --force-device-scale-factor=2 \
  --window-size=880,560 --screenshot=../features.png "file://$PWD/features.html"
```

Notes:

- `--window-size` must match the `html, body` width/height declared in each HTML file.
- `--force-device-scale-factor=2` renders at 2x for crisp retina display on GitHub.
- Fonts are local-only (`SF Mono` / `Menlo` fallback) so no network is needed to render.
