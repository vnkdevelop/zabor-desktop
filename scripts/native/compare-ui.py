import argparse
import json
from pathlib import Path
from PIL import Image, ImageChops, ImageStat


def summarize(reference, actual):
    difference = ImageChops.difference(reference, actual)
    channels = ImageStat.Stat(difference).mean
    data = difference.tobytes()
    changed = sum(max(data[index:index + 3]) > 16 for index in range(0, len(data), 3))
    return {
        'mean_absolute_rgb_error': round(sum(channels[:3]) / 3, 4),
        'pixels_over_16_percent': round(changed * 100 / (reference.width * reference.height), 4),
    }


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--reference', type=Path, required=True)
    parser.add_argument('--native', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    if args.output.exists():
        raise SystemExit('Comparison output already exists')
    references = sorted(args.reference.glob('react-updater-*.png'))
    if not references:
        raise SystemExit('No captures to compare')
    for reference_path in references:
        actual_path = args.native / reference_path.name.replace('react-', 'native-', 1)
        if not actual_path.exists():
            raise SystemExit('Native counterpart missing: ' + str(actual_path))
        with Image.open(reference_path) as reference, Image.open(actual_path) as actual:
            if reference.size != actual.size:
                raise SystemExit('Capture dimensions differ: ' + reference_path.name)
    args.output.mkdir(parents=True)
    results = []
    for reference_path in references:
        actual_path = args.native / reference_path.name.replace('react-', 'native-', 1)
        if not actual_path.exists():
            raise SystemExit('Native counterpart missing: ' + str(actual_path))
        reference = Image.open(reference_path).convert('RGB')
        actual = Image.open(actual_path).convert('RGB')
        if reference.size != actual.size:
            raise SystemExit('Capture dimensions differ: ' + reference_path.name)
        scale = reference.width / 420
        regions = {
            'titlebar': (0, 0, 420, 37),
            'heading': (20, 55, 400, 80),
            'body': (20, 105, 400, 172),
            'actions': (200, 183, 400, 223),
        }
        metrics = summarize(reference, actual)
        metrics['regions'] = {
            name: summarize(reference.crop(tuple(round(v * scale) for v in box)),
                            actual.crop(tuple(round(v * scale) for v in box)))
            for name, box in regions.items()
        }
        pair = Image.new('RGB', (reference.width * 2, reference.height))
        pair.paste(reference, (0, 0))
        pair.paste(actual, (reference.width, 0))
        pair.save(args.output / reference_path.name.replace('react-', 'comparison-', 1))
        difference = ImageChops.difference(reference, actual)
        difference.point(lambda value: min(255, value * 4)).save(args.output / reference_path.name.replace('react-', 'diff-', 1))
        results.append({'reference': reference_path.name, 'native': actual_path.name, **metrics})
    if not results:
        raise SystemExit('No captures to compare')
    report = {'status': 'measurement-only; visual and interaction acceptance is separate',
              'reference_left_native_right': True, 'captures': results}
    (args.output / 'comparison.json').write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding='utf-8')
    print(json.dumps({'captures': len(results), 'status': report['status'],
                      'maximum_mean_rgb_error': max(item['mean_absolute_rgb_error'] for item in results),
                      'report': str(args.output / 'comparison.json')}, ensure_ascii=False))


if __name__ == '__main__':
    main()
