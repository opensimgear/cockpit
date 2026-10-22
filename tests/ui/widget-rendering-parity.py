"""Compare portable typography at actual device resolution. Requires Pillow.

Run the real LVGL harness and widget-rendering-parity-native.mjs first. Browser
outline rasterization and LVGL bitmap resampling can each round a boundary by
one pixel, so the combined tolerance is two physical pixels. This checks text
geometry/ink, not live transport, input or hardware performance qualification.
"""
from pathlib import Path
import json
from PIL import Image, ImageFilter

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / '.research/widget-browser-reference'
browser = Image.open(OUT / 'active.png').convert('RGB')
native = Image.open(ROOT / '.research/lvgl-native/active.png').convert('RGB')
assert browser.size == native.size == (800, 480), 'Capture both renderers at the same physical resolution'
fixture = json.loads((ROOT / 'apps/companion-esp32/test/widget-lvgl.json').read_text())
frame = next(frame for frame in fixture['frames'] if frame['state'] == 'active')
results = []


def pixels(image):
    return image.get_flattened_data() if hasattr(image, 'get_flattened_data') else image.getdata()


for node in frame['nodes']:
    if node['kind'] != 'text':
        continue
    x, y, width, height = node['area']
    masks = []
    for image in [browser, native]:
        mask = Image.new('L', (width, height))
        # The fixture uses near-white foreground ink; this excludes its colored
        # bezels, backgrounds and indicators while retaining antialiased stems.
        mask.putdata([255 if min(pixel) > 120 and max(pixel) - min(pixel) < 60 else 0 for pixel in pixels(image.crop((x, y, x + width, y + height)))])
        masks.append(mask)
    a, b = masks
    counts = [sum(value > 0 for value in pixels(mask)) for mask in masks]
    assert min(counts) > 0, node['id'] + ': missing text'
    expanded = [mask.filter(ImageFilter.MaxFilter(5)) for mask in masks]
    outside = [sum(value > 0 and other == 0 for value, other in zip(pixels(mask), pixels(counterpart))) / count for mask, counterpart, count in [(a, expanded[1], counts[0]), (b, expanded[0], counts[1])]]
    boxes = [a.getbbox(), b.getbbox()]
    result = {'id': node['id'], 'text': node['text'], 'inkPixels': counts, 'bounds': boxes, 'outsideTwoPixels': outside}
    results.append(result)
    assert max(abs(left - right) for left, right in zip(*boxes)) <= 2, result
    assert max(outside) <= .02, result
    assert .5 <= counts[1] / counts[0] <= 2, result
(OUT / 'typography-parity.json').write_text(json.dumps({'physicalPixels': [800, 480], 'tolerancePixels': 2, 'layers': results}, indent=2) + '\n')
print(f'{len(results)} text layers match shared glyph placement within two physical pixels')
