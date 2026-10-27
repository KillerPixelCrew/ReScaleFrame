#!/usr/bin/env python3
"""Validate native RDR2 bundles and compare observed camera motion against copied NGX motion."""
import argparse
import json
import re
from pathlib import Path
import numpy as np


def resource(path):
    meta = json.loads(path.read_text())
    raw = path.with_suffix('.bin').read_bytes()
    if len(raw) != meta['row_pitch'] * meta['height']:
        raise ValueError(f'Incomplete readback: {path}')
    fmt = meta.get('footprint_format', meta['format'])
    types = {10: ('<f2', 4), 34: ('<f2', 2), 39: ('<f4', 1), 41: ('<f4', 1),
             28: ('u1', 4), 87: ('u1', 4), 24: ('<u4', 1), 26: ('<u4', 1)}
    if fmt not in types:
        return meta, None
    dtype, channels = types[fmt]
    data = np.frombuffer(raw, dtype=np.uint8).reshape(meta['height'], meta['row_pitch'])
    width_bytes = meta['width'] * np.dtype(dtype).itemsize * channels
    values = data[:, :width_bytes].copy().view(dtype).reshape(meta['height'], meta['width'], channels)
    return meta, values


def camera(raw, scalars):
    if len(raw) != 0x800:
        raise ValueError('Incomplete same-call camera')
    matrix = lambda offset: np.frombuffer(raw, '<f4', 16, offset).reshape(4, 4).astype(np.float64)
    scalar = lambda offset: float(np.frombuffer(raw, '<f4', 1, offset)[0])
    view, projection, world = matrix(0x40), matrix(0x80), matrix(0xc0)
    previous_view, previous_projection = matrix(0x140), matrix(0x180)
    if not all(np.isfinite(m).all() for m in (view, projection, world, previous_view, previous_projection)):
        raise ValueError('Non-finite camera matrices')
    w, h = scalar(0x67c), scalar(0x680)
    jitter = [scalars.get('Jitter.Offset.X'), scalars.get('Jitter.Offset.Y')]
    result = dict(near=scalar(0x630), far=scalar(0x634), fov_degrees=scalar(0x640), viewport=[w, h],
                  view_inverse_error=float(np.max(np.abs(view @ world - np.eye(4)))),
                  reset=scalars.get('Reset'))
    if all(v is not None for v in jitter):
        result['projection_jitter_error_pixels'] = [abs(-scalar(0x664)*w/2-jitter[0]), abs(scalar(0x668)*h/2-jitter[1])]
    # The prior scalar projection in this build carries current jitter. Keep the observed
    # jittered current projection for depth reconstruction, then compare unjittered positions.
    unjittered = projection.copy()
    prior_unjittered = previous_projection.copy()
    for p in (unjittered, prior_unjittered):
        p[2, 0] -= scalar(0x664)
        p[2, 1] -= scalar(0x668)
    return result, (view @ projection, view @ unjittered, previous_view @ prior_unjittered)


def reprojection(depth, motion, scalars, matrices, roi):
    w = int(scalars['DLSS.Render.Subrect.Dimensions.Width'])
    h = int(scalars['DLSS.Render.Subrect.Dimensions.Height'])
    dx, dy = (int(scalars.get(f'DLSS.Input.Depth.Subrect.Base.{a}', 0)) for a in ('X', 'Y'))
    mx, my = (int(scalars.get(f'DLSS.Input.MV.Subrect.Base.{a}', 0)) for a in ('X', 'Y'))
    if dx+w > depth.shape[1] or dy+h > depth.shape[0] or mx+w > motion.shape[1] or my+h > motion.shape[0]:
        return dict(status='active_rectangle_or_motion_extent_not_supported')
    x0, y0, rw, rh = roi if roi else (0, 0, w, h)
    if min(x0, y0) < 0 or rw <= 0 or rh <= 0 or x0+rw > w or y0+rh > h:
        return dict(status='roi_outside_active_rectangle')
    # Bounded sampling; source raw pixels remain available for full-resolution follow-up.
    yy, xx = np.mgrid[y0:y0+rh:4, x0:x0+rw:4]
    z = depth[dy+yy, dx+xx, 0].astype(np.float64)
    mv = motion[my+yy, mx+xx, :2].astype(np.float64)
    mv *= [scalars['MV.Scale.X'], scalars['MV.Scale.Y']]
    valid = np.isfinite(z) & (z > 0) & (z <= 1) & np.isfinite(mv).all(axis=2)
    clip = np.stack([(xx+.5)*2/w-1, 1-(yy+.5)*2/h, z, np.ones_like(z)], axis=-1)[valid]
    if not len(clip):
        return dict(status='no_valid_samples')
    world = clip @ np.linalg.inv(matrices[0])
    current, previous = world @ matrices[1], world @ matrices[2]
    usable = (abs(current[:, 3]) > 1e-9) & (abs(previous[:, 3]) > 1e-9)
    if not np.any(usable):
        return dict(status='no_valid_projected_samples')
    predicted = (previous[usable, :2]/previous[usable, 3, None] - current[usable, :2]/current[usable, 3, None]) * [w/2, -h/2]
    observed = mv[valid][usable]
    def residual(candidate):
        values = np.linalg.norm(candidate-observed, axis=1)
        return dict(median=float(np.median(values)), p90=float(np.percentile(values, 90)))
    magnitude = float(np.median(np.linalg.norm(predicted, axis=1)))
    return dict(status='numeric_comparison_only_static_geometry_not_classified', samples=len(predicted),
                predicted_median_pixels=magnitude, direction_inconclusive=magnitude < .05,
                previous_minus_current=residual(predicted), current_minus_previous=residual(-predicted))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('directory', type=Path)
    parser.add_argument('--roi', nargs=4, type=int, metavar=('X', 'Y', 'WIDTH', 'HEIGHT'))
    args = parser.parse_args()
    results = []
    for path in sorted(args.directory.glob('bundle-*.json')):
        if not re.fullmatch(r'bundle-\d+-\d+\.json', path.name):
            continue
        bundle = json.loads(path.read_text())
        pid, sequence = path.stem.split('-')[1:]
        row = dict(file=path.name, presentation=bundle['presentation'], draw_count=bundle['draw_count'],
                   draw_metadata=len(bundle['draws']), draw_overflow=bundle['draw_overflow'],
                   capture_timeout=bundle['capture_timeout'], write_failed=bundle['write_failed'],
                   unidentified_draw_shaders=sum(d['ps_crc32'] == '00000000' for d in bundle['draws']), temporal=[])
        for item in bundle['temporal']:
            prefix = f'input-{pid}-{sequence}-{item["index"]:03}-'
            captured = {}
            sample = dict(index=item['index'], frame=item['frame_counter_observation'], result=item['evaluation_result'],
                          refusals=item['refusals'], resources={})
            for meta_path in args.directory.glob(prefix+'*.json'):
                meta, pixels = resource(meta_path)
                role = meta['role']; captured[role] = pixels
                sample['resources'][role] = {k: meta[k] for k in ('width', 'height', 'format', 'footprint_format',
                    'row_pitch', 'command_list', 'generation', 'submission', 'queue', 'fence_value')}
                if pixels is not None and np.issubdtype(pixels.dtype, np.floating):
                    sample['resources'][role]['nonfinite_values'] = int(np.count_nonzero(~np.isfinite(pixels)))
            sample['missing_required_resources'] = [r for r in ('color', 'depth', 'motion', 'output') if r not in captured]
            if item['camera_valid']:
                sample['camera'], matrices = camera((args.directory/(prefix+'camera.bin')).read_bytes(), item['scalars'])
                if captured.get('depth') is not None and captured.get('motion') is not None:
                    try:
                        sample['reprojection'] = reprojection(captured['depth'], captured['motion'], item['scalars'], matrices, args.roi)
                    except (KeyError, np.linalg.LinAlgError) as error:
                        sample['reprojection'] = dict(status='unavailable', reason=str(error))
            row['temporal'].append(sample)
        results.append(row)
    if not results:
        raise ValueError('No completed bundle manifests')
    out = args.directory/'bundle-analysis.json'
    out.write_text(json.dumps(results, indent=2, allow_nan=False)+'\n')
    for row in results:
        print(row['file'], 'draws', row['draw_count'], 'inputs', len(row['temporal']),
              'unknown shaders', row['unidentified_draw_shaders'], 'timeout', row['capture_timeout'])


if __name__ == '__main__':
    main()
