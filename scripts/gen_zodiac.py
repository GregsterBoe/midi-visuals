#!/usr/bin/env python3
"""
Generate src/sketches/zodiac_points.rs from SVG silhouettes in assets/.

Usage:
    python3 scripts/gen_zodiac.py > src/sketches/zodiac_points.rs

Add new animals by placing an SVG in assets/ and adding an entry to FILES.
"""

from svgpathtools import svg2paths2
import os

ASSETS = os.path.join(os.path.dirname(__file__), '..', 'assets')
N = 90  # sample points per animal

# (svg filename, Rust CONST_NAME, display name)
FILES = [
    ('butterfly.svg',                      'BUTTERFLY', 'butterfly'),
    ('cock.svg',                            'COCK',      'cock'),
    ('elephant-alone-svgrepo-com.svg',      'ELEPHANT',  'elephant'),
    ('gecko-svgrepo-com.svg',               'GECKO',     'gecko'),
    ('horse.svg',                           'HORSE',     'horse'),
    ('pig-with-round-tail-svgrepo-com.svg', 'PIG',       'pig'),
    ('poisonous-cobra-svgrepo-com.svg',     'COBRA',     'cobra'),
    ('poisonous-spider-svgrepo-com.svg',    'SPIDER',    'spider'),
    ('sitting-cat-svgrepo-com.svg',         'CAT',       'cat'),
    ('sitting-squirrell-svgrepo-com.svg',   'SQUIRREL',  'squirrel'),
    ('tropical-frop-svgrepo-com.svg',       'FROG',      'frog'),
]


def sample_paths(paths, n_total):
    paths = [p for p in paths if p.length() > 1.0]
    if not paths:
        return []
    total_len = sum(p.length() for p in paths)
    samples = []
    for path in paths:
        n = max(4, int(round(n_total * path.length() / total_len)))
        for i in range(n):
            pt = path.point(i / n)
            samples.append((pt.real, pt.imag))
    return samples


def normalize(samples):
    xs = [s[0] for s in samples]
    ys = [s[1] for s in samples]
    cx = (max(xs) + min(xs)) / 2
    cy = (max(ys) + min(ys)) / 2
    scale = max(max(xs) - min(xs), max(ys) - min(ys)) / 2
    if scale == 0:
        return []
    # flip Y: SVG Y increases downward, screen Y increases upward
    return [((x - cx) / scale, -(y - cy) / scale) for x, y in samples]


out = []
out.append("// Auto-generated from assets/ SVGs — run scripts/gen_zodiac.py to regenerate")
out.append("// Points are normalised to [-1, 1], Y-flipped to match screen coordinates.")
out.append("")

animal_list = []

for filename, const_name, lower_name in FILES:
    full_path = os.path.join(ASSETS, filename)
    if not os.path.exists(full_path):
        print(f"# SKIP {filename}: not found", flush=True)
        continue
    paths, _, _ = svg2paths2(full_path)
    samples = sample_paths(paths, N)
    normed = normalize(samples)
    if not normed:
        print(f"# SKIP {filename}: no usable paths", flush=True)
        continue
    animal_list.append((lower_name, const_name))
    out.append(f"pub const {const_name}: &[(f32, f32)] = &[")
    for x, y in normed:
        out.append(f"    ({x:.4f}_f32, {y:.4f}_f32),")
    out.append("];")
    out.append("")

out.append("pub const ANIMALS: &[(&str, &[(f32, f32)])] = &[")
for lower, upper in animal_list:
    out.append(f'    ("{lower}", {upper}),')
out.append("];")
out.append("")

print("\n".join(out))
