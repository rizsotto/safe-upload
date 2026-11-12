# 0002: Predict the fit-to-size settings instead of searching

- Status: proposed
- Blocks: nothing. Option B is built on branch `m2/fit-predict` so the
  human can judge it with code and numbers.
- SPEC refs: NFR-1, FR-3, FR-4, section 9.3

## Question

The default target is now 5 MB (SPEC 16.3). NFR-1 asks for a 12 MP JPEG to
5 MB in under 2 s, and says fit-to-size should predict the encoder settings
so the common case needs one encode. Can we predict the scale and quality
from cheap features, and how good is the prediction?

## Context

The M1 search (SPEC 9.3 before this change) encodes quality 92 and 40 in
parallel, then up to two rounds of three guesses, at each scale. A 12 MP
JPEG to 2 MB took 1.9 to 4.3 s, mostly from 5 to 8 full mozjpeg encodes.

A full mozjpeg encode is the expensive step. With our settings (trellis on,
`optimize_scans` off, SIMD) it costs about 23 ms per MP at quality 40,
39 ms/MP at 75 and 60 ms/MP at 92 (median; range 30 to 144 ms/MP at 92).
So one 12 MP encode at quality 92 takes 0.4 to 1.7 s. Other steps, per
photo: glycin decode 90 to 560 ms (about 12 ms/MP), sRGB conversion about
8 ms, Lanczos3 resize about 26 ms per source MP.

### Photos

20 photos from Wikimedia Commons, all CC0 (license from the Commons API,
`LicenseShortName`). They were kept in a scratch folder, not in the repo.
Commons rate-limited full-size downloads, so 10 of the 20 (marked
"thumb") are Commons' 3840 px wide thumbnails, not the originals. Commons
re-encodes those, which smooths a little of the camera noise, so they may
be a bit easier to predict than camera files.

| Name | Kind | Size | Source (Commons file page) |
|---|---|---|---|
| landscape-gorge | landscape | 6000x4000 | `File:2018_Yuntai_Mountain_Quanpu_Gorge_03.jpg` |
| landscape-temple | landscape, haze | 6000x4000 | `File:2018_Yuntai_Mountain_Temple.jpg` |
| portrait-sundress | portrait | 4928x3264 | `File:A_Woman_in_a_Red_Sundress.jpg` |
| portrait-smile | portrait, smooth | 3746x3746 | `File:Always_smile_(Unsplash).jpg` |
| portrait-fishmonger | portrait | 3765x2581 | `File:Fishmonger_smiling.jpg` |
| night-city | night | 4032x3024 | `File:Bloemfontein_at_night.jpg` |
| night-stars | night, noisy sky | 4896x3672 | `File:Cévennes_France_night_sky_with_stars_01.jpg` |
| night-chateau | night, noisy | 5855x3747 | `File:Chateau_Frontenac_illuminated_at_night_in_Quebec_City.jpg` |
| text-newspaper | document scan | 3560x5296 | `File:Banater_Deutsche_Zeitung_vom_10._November_1936.jpg` |
| text-stand | text in a photo | 4032x2268 | `File:Carmel_Library_newspaper_stand.jpg` |
| text-archive | documents on a table | 3840x2555 (thumb) | `File:Archive_de_la_revue_"Teny_Soa",_musée_de_l'express_de_Madagascar.jpg` |
| foliage-alder | foliage | 3840x1920 (thumb) | `File:Alder_forest.jpg` |
| foliage-badgrund | foliage | 3840x2560 (thumb) | `File:Bad_Grund_(Mai)_1.jpg` |
| foliage-meadow | meadow | 3840x2552 (thumb) | `File:Blumenwiese_Gnalp_2_–_WAF_300_J_FL.jpg` |
| sky-clouds | sky | 3840x2560 (thumb) | `File:Atmosphere_Sky_Clouds.jpg` |
| sky-buea | sky | 3840x5120 (thumb) | `File:Blue_sky_in_Buea.jpg` |
| indoor-reading | indoor | 3840x2560 (thumb) | `File:BM_Reading_Room_November_2024.jpg` |
| indoor-kitchen | indoor | 3840x2880 (thumb) | `File:176_Nile_Street_dining_area_kitchen.jpg` |
| indoor-conference | indoor | 3840x2560 (thumb) | `File:AM140-01_interior_-_conference_room_back.jpg` |
| object-car | object in forest | 3840x2560 (thumb) | `File:Antique_car_in_forest.jpg` |

All are under `https://commons.wikimedia.org/wiki/`. foliage-alder is
7.4 MP, the rest are 9 to 24 MP.

### What we measured

With our own pipeline (glycin decode, orientation, `color::to_srgb`,
Lanczos3 resize, `JpegEncoder`), for every photo: output size at qualities
40, 45, ..., 90, 92 and scales 1.0, 0.7, 0.5, 0.35 (960 full encodes), plus
cheap samples encoded at the same settings. Size spread is wide: at quality
92 the photos give 0.03 to 0.52 bytes per pixel, so a fixed curve cannot
work.

Sample methods, each predicting full size as sample bytes times the pixel
ratio. To compare methods fairly, the errors below are after one global
factor fitted per method (the median of actual over predicted). For the
mosaics that factor is 0.97 to 1.0; the code uses no factor (1.0), and
the 95% aim and the correction cover the rest. The downscaled samples
need 0.63 to 0.76, which is part of why they are no good:

| Sample | Sample pixels | Build + one encode | Median error | 90th pct | Worst |
|---|---|---|---|---|---|
| Downscale to 1/4 edge (Triangle) | 1/16 | 82 + 38 ms | 19% | 60% | 172% |
| Downscale to 1/8 edge | 1/64 | 34 + 10 ms | 30% | 100% | 261% |
| Mosaic 8x8 tiles of 64 px | 0.26 MP | 1 + 11 ms | 3.4% | 14% | 39% |
| Mosaic 16x16 tiles of 32 px | 0.26 MP | 2 + 11 ms | 2.3% | 13% | 25% |
| Mosaic 8x8 tiles of 128 px | 1 MP | 5 + 41 ms | 1.9% | 7.7% | 30% |
| Mosaic 16x16 tiles of 64 px | 1 MP | 5 + 41 ms | 1.4% | 6.1% | 17% |

(Errors over all 20 photos, 4 scales and 12 qualities.) A downscaled
sample has more detail per pixel than the full image, and how much more
depends on the photo, so it predicts badly. A mosaic of full-resolution
tiles keeps the detail per pixel: JPEG codes each block on its own, so
bytes per pixel carry over. The error does not depend on quality. Most
large errors are over-predictions (the output is smaller than predicted),
on photos with big flat areas (sky, smooth skin, starry sky). The worst
under-prediction is a newspaper scan, where the regular grid lines up with
the column layout.

Gradient energy and source bytes per pixel were also computed. They rank
the photos roughly, but a sample encode measures the encoder directly, so
they were not pursued.

Size against scale: output size grows about as scale^1.7 (median 1.63 at
quality 40, 1.69 at 92; range 0.9 to 2.8), not with the pixel count.

### End-to-end test of option B

`fit()` from this branch against the M1 search, on the 20 photos at their
own size. "Encodes" counts full-image encodes only. Times are for fit
alone, release build, Ryzen 7 PRO 7840HS, while other jobs loaded the
machine (load average 13 to 55), so they are noisy. The "new" rows are
the final code (up-try below 80%). With an 85% bound it was 72 of 80
with one encode and 8 with two: a few photos spent a full encode to go
from 82-84% up by one quality step.

| Target | Algorithm | 1 encode | 2 encodes | More | Mean time | Over target | Median size/target |
|---|---|---|---|---|---|---|---|
| 5 MB | new | 20 | 0 | 0 | 1.06 s | 0 | 0.45 |
| 5 MB | M1 | 0 | 17 | 3 (up to 5) | 1.35 s | 0 | 0.45 |
| 2 MB | new | 19 | 1 | 0 | 0.93 s | 0 | 0.92 |
| 2 MB | M1 | 0 | 6 | 14 (up to 8) | 1.80 s | 0 | 0.95 |
| 1 MB | new | 19 | 1 | 0 | 0.75 s | 0 | 0.91 |
| 1 MB | M1 | 0 | 3 | 17 (up to 8) | 2.11 s | 0 | 0.94 |
| 500 KB | new | 16 | 4 | 0 | 0.75 s | 0 | 0.92 |
| 500 KB | M1 | 0 | 2 | 18 (up to 8) | 2.24 s | 0 | 0.96 |

At 5 MB the median output is about 45% of the target at quality 92, full
size: nearly every photo already fits at Q_MAX, so the 5 MB row mostly
tests that path, not the prediction. Both algorithms give the same file
there; the old one already needed one round. Every new output under 80%
of the target is quality 92 at full size, where nothing better is
allowed. The new outputs are a little smaller (median 0.92 against 0.95)
because they aim at 95% and stop.

Same photos scaled to 12 MP, fit only, median of 3 runs on 6 photos:
5 MB 0.77 s (0.5 to 2.6 s), 2 MB 1.29 s (0.6 to 1.4 s); M1 search 0.84 s
and 1.45 s (up to 5.2 s). The 2.6 s case is night-chateau: its one quality
92 encode alone takes that long under load.

### Whole CLI, release build

`safe-upload --max-size` on a real 12.2 MP camera JPEG (night-city) and a
24 MP one (landscape-gorge), this branch against M1 (commit 158c3fa),
6 interleaved runs each, same machine. The load average was 32 to 36, so
the fastest run is the fairest number; all runs are in the scratch data.

| Photo | Target | New, fastest (all runs) | M1, fastest (all runs) |
|---|---|---|---|
| night-city, 12 MP | 5 MB | 1.55 s (1.55 to 2.97) | 1.76 s (1.76 to 2.88) |
| night-city, 12 MP | 2 MB | 1.49 s (1.49 to 2.61) | 2.53 s (2.53 to 5.31) |
| landscape-gorge, 24 MP | 5 MB | 3.47 s (3.47 to 5.00) | 7.31 s (7.31 to 10.58) |
| landscape-gorge, 24 MP | 2 MB | 2.32 s (2.32 to 3.35) | 6.23 s (6.23 to 7.85) |

The CLI time includes the sandboxed decode, the input scan, writing and
verifying the output. These runs are from before the review fixes (85%
up-try bound); for these two photos both versions do one full encode. The
machine never went idle (load average 13 or more), so the table still
needs a run on an idle machine before NFR-1 is called met.

## Options

### A. Keep the search, start it from a predicted bracket
Encode the sample to guess a quality, then run the M1 rounds around it.
Pros: small change. Cons: still two full encodes or more in each case,
because the bracket needs a point on each side.

### B. Predict from a tile mosaic, encode once, correct with the measured ratio (built)
Build a mosaic of 8x8 tiles of 64 px, one from the middle of each cell of
an 8x8 grid, at the output scale. Binary-search the quality on the mosaic
(about 2 to 12 encodes of 0.26 MP, 20 to 130 ms). Predict the full size as
header bytes plus mosaic bytes times the pixel ratio. Aim at 95% of the
target. If even quality 40 is predicted too big, shrink the scale by the
1.7 power law and sample again. Encode the full image once. If it is over
the target, divide the prediction by the measured ratio and pick a lower
quality; if that is still over, binary-search between the best quality
that fits and the lowest that does not, or scale down if even quality 40
is too big. If the first output is under 80%, try one higher quality the
same way and keep whichever fits. Small images (at most 512x512 after scaling)
are their own sample, so their sample encode is the output.
Pros: one full encode in 74 of 80 cases and two in 6 of 80, measured
(see the table). The worst case is bounded by design at 8 full encodes per
scale: the guess, one correction, and a binary search over at most 52
qualities. Never over target. Simple: one loop, no parallel threads. Works for PNG
too (the correction handles PNG's flatter size curve).
Cons: the sample work is sequential (about 2 to 12 small encodes). The
prediction error is larger for big flat areas and grid-like documents, which
costs a second encode there.

### C. Larger mosaic (16x16 tiles of 64 px, 1 MP)
Pros: about half the prediction error. Cons: each sample encode costs
about 40 ms instead of 11, so the search on the sample costs 100 to 350 ms,
more than the second full encode it saves in a few cases.

### D. Fixed per-quality curve scaled by a complexity measure
Pros: no sample encodes. Cons: bytes per pixel at a given quality varies
17x across the photos, and a cheap feature cannot pin that down to a few
percent. It would need correction encodes most of the time.

## Preferred option and why

B. The data clearly supports it: the tile mosaic predicts the full size
within a few percent for most photos, at about 1/40 of the cost of a full
encode. Measured: 1 full encode in 74 of 80 runs, 2 in 6 of 80. Worst
case, bounded by design: 8 full encodes per scale. The hard rule holds:
the loop only returns an encode that it measured to be within the target,
and it reports "cannot fit" only after quality 40 at the floor was encoded
and was too big. SPEC 9.3 is updated to match.

The up-try bound is 80%, not FR-3's 85%, so a result of 80-85% is kept
instead of spending another full encode on one quality step. FR-3 says
"should", so this fits it, but the human should confirm the trade.

NFR-1 is close but not always met: the 12 MP photo took 1.5 s at best
for both 5 MB and 2 MB on a busy machine. With one encode left, the time is
decode plus one mozjpeg encode at the chosen quality. For very detailed or
noisy 12 MP photos at quality 92, that encode alone takes 1.5 s or more.
A follow-up could measure on a quiet machine, and look at trellis
quantization (the main encode cost) for large images. That is a separate
question from this record.

## Sources

- Raw data (CSV, not in the repo): photos.csv (per-photo features and
  timings), sizes.csv (full encodes), samples.csv (sample encodes),
  eval-g8.csv and eval-12mp*.csv (end-to-end runs), cli-times.txt.
- Wikimedia Commons file pages listed above, all CC0.
- mozjpeg 0.10.13 (mozjpeg-sys 2.2.3), image 0.25.10, as in `Cargo.lock`.

## Human decision
