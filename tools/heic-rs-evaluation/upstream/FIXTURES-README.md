# heic-rs test fixtures

**Everything in this directory is generated.** `scripts/make-fixtures.sh` builds it
from PNGs synthesised by `scripts/gen-pngs.py`, a pure-stdlib Python 3 script that
writes every pixel itself (`zlib`, `struct`, `sys`, `os` only -- no PIL, no numpy).
**No third-party image data is included**, nothing is downloaded, and no fixture was
hand-edited.

The fixtures are licensed **MIT OR Apache-2.0**, the same as the crate.

## Regenerating

```sh
bash scripts/make-fixtures.sh
```

Re-running is safe: every output is rewritten from scratch. macOS only -- it needs
Apple's `sips`, which is what encodes the HEIC and produces the reference decode.

## How each fixture is made

1. `scripts/gen-pngs.py` writes a source PNG (8-bit RGB, colour type 2, non-interlaced)
   into a temp directory.
2. `sips -s format heic <png> --out <name>.heic` encodes it.
3. `sips -s format png <name>.heic --out <name>.ref.png` decodes that HEIC back.

Step 3 matters: `<name>.ref.png` is **Apple's decode of our HEIC**, not the original
source PNG. It is the ground truth to compare `heic-rs` output against. HEVC is lossy
and the pipeline is 4:2:0, so expect a few code values of drift -- see
`GROUND-TRUTH.md` for measured spot values and a suggested tolerance.

`GROUND-TRUTH.md` holds the parsed ISOBMFF box structure of every `.heic`: brands,
primary item, item list, grid geometry, property values, and `iloc` layout. Use it
to write exact parser assertions.

## The fixtures

| File | Dimensions | What it depicts | Structure |
|---|---|---|---|
| `flat-white-16.heic` | 16x16 | Solid white (255,255,255) | single `hvc1` item |
| `flat-64.heic` | 64x64 | Solid RGB (200,30,60) | single `hvc1` item |
| `checker-64.heic` | 64x64 | 8px black/white checkerboard, top-left cell white | single `hvc1` item |
| `rgb-strips-96.heic` | 96x32 | Three 32px vertical strips: pure red, pure green, pure blue | single `hvc1` item |
| `gradient-512.heic` | 512x512 | Red ramps left to right, green ramps top to bottom, blue constant 128 | single `hvc1` item |
| `checker-1024.heic` | 1024x1024 | 64px black/white checkerboard, top-left cell white | `grid`, 2 rows x 2 cols of 512x512 tiles |
| `photo-2048.heic` | 2048x1536 | Smooth synthetic multi-colour field built from superposed sinusoids | `grid`, 3 rows x 4 cols of 512x512 tiles |
| `rotated-90.heic` | 1536x2048 | `photo-2048` put through `sips -r 90` | `grid`, 4 rows x 3 cols; rotation is baked into the pixels, `irot` is still 0 |
| `with-exif.heic` | 2048x1536 | `photo-2048` with description, copyright and artist set via `sips -s` | `grid`, 3 rows x 4 cols, plus an `Exif` item and an XMP `mime` item |

Each `.heic` has a matching `.ref.png` at the same dimensions (8-bit RGB, colour
type 2).

## Caveats for test authors

* `irot` is present in **every** fixture with **angle 0**. Presence alone does not
  mean the image is rotated.
* `sips -r 90` re-encodes pixels instead of writing `irot`, so `rotated-90.heic`
  exercises a transposed grid, not orientation metadata. No fixture here has a
  non-zero `irot` or any `imir`.
* `sips -r 90` also injects an `Exif` item as a side effect, so `rotated-90.heic`
  has one too -- it is not a metadata-free file.
* No fixture has alpha; `auxC` appears nowhere.
* These bytes come from the host's HEVC encoder. A different macOS release may
  produce different offsets, lengths and level_idc values -- regenerate
  `GROUND-TRUTH.md` alongside the fixtures if you re-run the script on a new OS.
