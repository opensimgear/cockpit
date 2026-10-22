"""Build the software acceptance target against the same pinned LVGL library.

The caller supplies the exact verified native LVGL/font objects. This target
replaces board/task/flash I/O only and never qualifies physical ESP32 hardware.
"""
from pathlib import Path
import json
import os
import subprocess


def build(app, out, flags, font, library):
    sources = [
        'core/companion.c', 'core/widget.c', 'core/raster.c',
        'render/lvgl_backend.c', 'render/lvgl_pool.c', 'render/widget_fonts.c',
        'render/widget_source.c', 'render/widget_state.c', 'render/widget_draw.c',
        'render/panel_draw.c', 'network/input.c', 'network/publication.c',
        'network/json.c', 'network/render_profile.c',
        'network/panel_source.c', 'network/scene_bank.c', 'network/serial_json.c',
        'network/settings.c', 'third_party/cjson/cJSON.c', 'test/software_client.c',
    ]
    binary = out / 'software-client'
    includes = ['test/include', 'test', 'network']
    subprocess.run([
        os.environ.get('CC', 'gcc'), *flags, '-Wall', '-Wextra', '-Werror',
        '-DOSG_BOARD_PROFILE="software-lvgl"',
        *['-I' + str(app / name) for name in includes],
        *[str(app / name) for name in sources], str(font), str(library),
        '-lm', '-lcrypto', '-o', str(binary),
    ], check=True)
    result=subprocess.run([str(binary)], input='{"command":"quit","requestId":1}\n', text=True,
                          check=True, timeout=10, capture_output=True, env={**os.environ, 'UBSAN_OPTIONS':'halt_on_error=1:print_stacktrace=1'})
    evidence=json.loads(result.stdout)
    assert evidence['armed'] is False and evidence['phase']=='offline' and evidence['frames']==0
    (out/'software-client-smoke.json').write_text(json.dumps(evidence,indent=2)+'\n')
    print('Actual firmware/LVGL software-client sanitizer smoke passed without creating authority.')
    return binary


if __name__ == '__main__':
    import argparse
    parser = argparse.ArgumentParser()
    parser.add_argument('--lvgl', type=Path, required=True)
    parser.add_argument('--font', type=Path, required=True)
    parser.add_argument('--include', type=Path, required=True)
    parser.add_argument('--out', type=Path, required=True)
    arguments = parser.parse_args()
    app = Path(__file__).resolve().parents[1]
    arguments.out.mkdir(parents=True, exist_ok=True)
    flags = ['-std=c11', '-O1', '-g', '-fsanitize=address,undefined', '-fno-omit-frame-pointer',
             '-DLV_CONF_INCLUDE_SIMPLE', '-DLV_KCONFIG_IGNORE', '-I'+str(arguments.include)]
    flags += ['-I'+str(app / name) for name in ['render','core','boards','third_party/cjson']]
    build(app, arguments.out.resolve(), flags, arguments.font.resolve(), arguments.lvgl.resolve())
