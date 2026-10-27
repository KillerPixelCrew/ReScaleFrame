#!/usr/bin/env python3
"""Validate bounded native readbacks and export their actual pixels for inspection."""
import argparse
import json
import re
from pathlib import Path
import numpy as np
from PIL import Image


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('directory', type=Path)
    parser.add_argument('--all-previews', action='store_true')
    args = parser.parse_args()
    rows = []
    previous = {}
    first = {}
    paths = [p for p in sorted(args.directory.glob('native-*.json'))
             if re.fullmatch(r'native-\d+-\d+-\d+\.json', p.name)]
    for index, path in enumerate(paths):
        meta = json.loads(path.read_text())
        raw = path.with_suffix('.bin').read_bytes()
        w, h, pitch = meta['width'], meta['height'], meta['row_pitch']
        if len(raw) != pitch * h or pitch < w * 4:
            raise ValueError(f'Incomplete readback: {path}')
        pixels = np.frombuffer(raw, dtype=np.uint8).reshape(h, pitch)[:, :w*4].reshape(h, w, 4)
        if meta['format'] in (87, 91):
            rgba = pixels[:, :, [2, 1, 0, 3]]
        elif meta['format'] in (28, 29):
            rgba = pixels
        elif meta['format'] == 24:
            packed = pixels.copy().view('<u4').reshape(h, w)
            rgba = np.stack([(packed & 1023) * 255 // 1023, ((packed >> 10) & 1023) * 255 // 1023,
                             ((packed >> 20) & 1023) * 255 // 1023, (packed >> 30) * 85], axis=-1).astype(np.uint8)
        else:
            raise ValueError(f'Unimplemented output format {meta["format"]}')
        # HDR export preserves encoded values; this preview is not a tone-mapped HDR measurement.
        key = (path.name.split('-')[1], meta['sequence'])
        last_in_sequence = index + 1 == len(paths) or paths[index + 1].stem.rsplit('-', 1)[0] != path.stem.rsplit('-', 1)[0]
        if args.all_previews or key not in previous or last_in_sequence:
            Image.fromarray(rgba).save(path.with_suffix('.png'), compress_level=1)
        row = dict(meta, file=path.name, rgb_mean=rgba[:, :, :3].mean(axis=(0, 1)).tolist())
        rgb = pixels.copy().view('<u4').reshape(h, w) & 0x3fffffff if meta['format'] == 24 else pixels[:, :, :3]
        if key not in first:
            first[key] = rgb.copy()
        if key in previous:
            diff = rgb != previous[key]
            if diff.ndim == 3:
                diff = np.any(diff, axis=2)
            yy, xx = np.where(diff)
            row['changed_rgb_pixels'] = int(len(xx))
            row['changed_bounds'] = [int(xx.min()), int(yy.min()), int(xx.max()), int(yy.max())] if len(xx) else None
        cumulative = rgb != first[key]
        if cumulative.ndim == 3:
            cumulative = np.any(cumulative, axis=2)
        row['changed_since_scene_pixels'] = int(np.count_nonzero(cumulative))
        previous[key] = rgb.copy()
        rows.append(row)
    if not rows:
        raise ValueError('No completed native readbacks')
    (args.directory/'native-analysis.json').write_text(json.dumps(rows, indent=2)+'\n')
    for r in rows:
        print(r['sequence'], r['draw'], r['ps_crc32'], r.get('changed_rgb_pixels'), r.get('changed_bounds'))


if __name__ == '__main__':
    main()
