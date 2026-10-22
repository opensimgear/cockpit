"""Compare source-driven LVGL with the actual WidgetView in dev Cockpit.

Two physical pixels cover rasterizer edge/glyph rounding. Solid colors may
differ by one RGB565 channel step. Antialiased edge pixels may blend that color
with a nearby dominant background; compare their color footprint at >=25%
coverage. Uniform interiors still require the RGB565 tolerance. Check each
significant color footprint in
each widget in both directions, so a large background cannot hide a missing
indicator, gauge, border or label. Antialiased edge shades are not solid colors.
"""
from collections import Counter
import json
from pathlib import Path
import sys
from PIL import Image, ImageChops, ImageFilter

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / '.research/widget-source-browser-reference'
fixture = json.loads((ROOT / 'apps/companion-esp32/test/widget-draw.json').read_text())
STEPS = (1, 1, 1)


def rgb565(pixel):
    return pixel[0] >> 3, pixel[1] >> 2, pixel[2] >> 3


def values(image):
    return image.get_flattened_data() if hasattr(image, 'get_flattened_data') else image.getdata()


def canonical(image):
    result = image.copy()
    # Glyph rasterizers produce different AA shades. Keep the same foreground
    # ink threshold as the independent portable typography comparison.
    result.putdata([rgb565((255, 255, 255) if min(p) > 120 and max(p) - min(p) < 60 else p) for p in values(image)])
    return result


def close(a, b):
    return all(abs(x - y) <= step for x, y, step in zip(a, b, STEPS))


def match(image, color, expected_colors):
    difference = ImageChops.difference(image, Image.new('RGB', image.size, color))
    masks = [channel.point(lambda value, step=step: 255 if value <= step else 0) for channel, step in zip(difference.split(), STEPS)]
    accepted = ImageChops.multiply(ImageChops.multiply(masks[0], masks[1]), masks[2])
    low, high = image.filter(ImageFilter.MinFilter(3)), image.filter(ImageFilter.MaxFilter(3))
    pixels = list(values(image))
    interior = {p for p, minimum, maximum in zip(pixels, values(low), values(high)) if minimum == maximum}
    backgrounds = [p for p, _ in Counter(pixels).most_common() if p in interior and p[0] * 8 + p[1] * 4 + p[2] * 8 < 360][:3]
    edges = list(zip(values(low), values(high)))
    result = list(values(accepted))
    foregrounds = [color, *[p for p in expected_colors if close(color, p)]]
    for index, (pixel, (minimum, maximum)) in enumerate(zip(pixels, edges)):
        if result[index] or minimum == maximum:
            continue
        for background in backgrounds:
            for foreground in foregrounds:
                low_coverage, high_coverage = .25, 1.03
                for p, b, f, step in zip(pixel, background, foreground, STEPS):
                    vector = f - b
                    if not vector:
                        if abs(p - b) > step:
                            high_coverage = -1
                            break
                        continue
                    left, right = sorted(((p - b - step) / vector, (p - b + step) / vector))
                    low_coverage, high_coverage = max(low_coverage, left), min(high_coverage, right)
                if low_coverage <= high_coverage:
                    result[index] = 255
                    break
            if result[index]:
                break
    accepted.putdata(result)
    return accepted.filter(ImageFilter.MaxFilter(5))


def check_controls():
    background = Image.new('RGB', (20, 20), rgb565((13, 23, 32)))
    foreground = rgb565((255, 0, 0))
    correct = background.copy()
    correct.paste(foreground, (5, 5, 10, 10))
    shifted = background.copy()
    shifted.paste(foreground, (12, 5, 17, 10))
    wrong_color = background.copy()
    wrong_color.paste(rgb565((0, 255, 0)), (5, 5, 10, 10))
    assert match(correct, foreground, [foreground]).getpixel((7, 7))
    assert not match(background, foreground, [foreground]).getpixel((7, 7)), 'Missing features must fail'
    assert not match(shifted, foreground, [foreground]).getpixel((7, 7)), 'Shifts beyond tolerance must fail'
    assert not match(wrong_color, foreground, [foreground]).getpixel((7, 7)), 'Different paint colors must fail'


check_controls()
if '--check-controls' in sys.argv:
    print('Visual tolerance rejects missing, displaced and recolored features')
    sys.exit(0)


results = []
failures = []
for frame in fixture['frames']:
    name = frame['name']
    browser = Image.open(OUT / (name + '.png')).convert('RGB')
    native = Image.open(ROOT / '.research/lvgl-native' / ('source-' + name + '.png')).convert('RGB')
    assert browser.size == native.size == (800, 480)
    for index, (widget, context) in enumerate(zip(fixture['widgets'], frame['contexts'])):
        bounds = widget['bounds']
        x, y = round(bounds['x'] * 800), round(bounds['y'] * 480)
        width, height = round(bounds['width'] * 800), round(bounds['height'] * 480)
        images = [canonical(image.crop((x, y, x + width, y + height))) for image in [browser, native]]
        whole = (0, 0, width, height)
        palette = [(rgb565((13, 23, 32)), whole, True), (rgb565((255, 255, 255)), whole, True)]
        properties = {p['name']: widget['properties'].get(p['name'], p['default']) for p in widget['definition']['properties']}
        for layer_index, layer in enumerate(widget['definition']['layers']):
            b = layer['bounds']
            region = (b['x'] * width - 2, b['y'] * height - 2, (b['x'] + b['width']) * width + 2, (b['y'] + b['height']) * height + 2)
            paint = {**layer, **layer.get('states', {}).get('normal', {}), **layer.get('states', {}).get(context['state'], {})}
            opacity = paint.get('opacity', 1)
            if isinstance(opacity, dict):
                opacity = properties[opacity['property']]
            # Catalog alpha decoration is the first bezel over the canvas.
            # Reject a changed fixture until its new compositing is modeled.
            assert opacity == 1 or layer_index == 0 and layer['kind'] == 'rectangle'

            def paint_color(color):
                return rgb565(tuple(round(opacity * p + (1 - opacity) * bg) for p, bg in zip(color, (13, 23, 32))))

            if layer['kind'] == 'indicator':
                palette.extend([(paint_color((66, 81, 90)), region, True), (paint_color((99, 223, 208)), region, True)])
            if layer['kind'] == 'gauge':
                palette.extend([(paint_color((48, 67, 77)), region, True), (paint_color((99, 223, 208)), region, True)])
            for field in ['fill', 'stroke']:
                color = paint.get(field)
                if isinstance(color, dict):
                    color = properties[color['property']]
                if color:
                    palette.append((paint_color(tuple(int(color[i:i + 2], 16) for i in [1, 3, 5])), region, field != 'fill' or layer['kind'] != 'rectangle'))
        checked = []
        for side, image in enumerate(images):
            low, high = image.filter(ImageFilter.MinFilter(3)), image.filter(ImageFilter.MaxFilter(3))
            interiors = {pixel for pixel, minimum, maximum in zip(values(image), values(low), values(high)) if minimum == maximum}
            counts = Counter(values(image))
            for color, count in counts.items():
                regions = [region for p, region, edge_color in palette if close(color, p) and (edge_color or color in interiors)]
                if count < 32 or color not in interiors and not regions:
                    continue
                # An AA glyph shade can coincide with an unrelated gauge's
                # track color. Attribute paint colors to their source layers.
                eligible = [i for i, pixel in enumerate(values(image)) if pixel == color and (not regions or any(x1 <= i % width < x2 and y1 <= i // width < y2 for x1, y1, x2, y2 in regions))]
                count = len(eligible)
                if count < 32:
                    continue
                matched = match(images[1 - side], color, [p for p, _, _ in palette])
                accepted = list(values(matched))
                outside = sum(not accepted[i] for i in eligible)
                result = {'color': color, 'pixels': count, 'outsideTwoPixels': outside / count, 'side': side}
                checked.append(result)
                if outside / count > .05:
                    failures.append({'frame': name, 'widget': index, **result})
        assert len(checked) >= 2, (name, index, checked)
        results.append({'frame': name, 'widget': index, 'definition': widget['definition']['id'], 'state': context['state'], 'colors': checked})
(OUT / 'source-parity.json').write_text(json.dumps({'physicalPixels': [800, 480], 'tolerancePixels': 2, 'colorChannelTolerance': STEPS, 'widgets': results}, indent=2) + '\n')
assert not failures, failures[:10]
print(f'{len(results)} source-driven widget appearances match shared geometry, color and ink footprints')
