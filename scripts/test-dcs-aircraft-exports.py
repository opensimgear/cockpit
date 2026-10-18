#!/usr/bin/env python3
"""Test bundled aircraft exports against an installed pinned DCS-BIOS runtime.

No game, device, service or socket is started. All writes go to disposable files.
Example: --bios-source '.../Saved Games/DCS' --dll '.../DCSWorld/bin/lua.dll'
"""
import argparse
import contextlib
import hashlib
import zipfile
import importlib.util
import json
from pathlib import Path
import tempfile

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('lua_runner', ROOT / 'scripts/dcs-aircraft-lua.py')
runner = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runner)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    source = parser.add_mutually_exclusive_group(required=True)
    source.add_argument('--bios-source', type=Path)
    source.add_argument('--archive', type=Path, help='Checksum-pinned official DCS-BIOS 0.11.7 archive')
    runtime = parser.add_mutually_exclusive_group(required=True)
    runtime.add_argument('--dll', type=Path)
    runtime.add_argument('--lua', type=Path)
    args = parser.parse_args()
    with contextlib.ExitStack() as cleanup:
        if args.archive:
            data = args.archive.read_bytes()
            expected = '3272bc07210fa6c560f115cfb5dc8881accf7af5e7a5e218e2c46e1fd89e3292'
            if hashlib.sha256(data).hexdigest() != expected:
                raise ValueError('The archive is not the pinned DCS-BIOS 0.11.7 release')
            destination = Path(cleanup.enter_context(tempfile.TemporaryDirectory(prefix='osg-bios-api-')))
            with zipfile.ZipFile(args.archive) as archive:
                for entry in archive.infolist():
                    parts = Path(entry.filename).parts
                    if not parts or parts[0] != 'DCS-BIOS' or entry.is_dir():
                        continue
                    if '..' in parts or Path(entry.filename).is_absolute():
                        raise ValueError('Unsafe archive path')
                    path = destination / 'Scripts' / entry.filename
                    path.parent.mkdir(parents=True, exist_ok=True)
                    path.write_bytes(archive.read(entry))
            args.bios_source = destination
        run_cases(args)


def run_cases(args):
    for selected in [[], ['MH-6J'], ['AH-6J'], ['OH-6A'], ['MH-6J', 'AH-6J', 'OH-6A']]:
        with tempfile.TemporaryDirectory(prefix='osg-aircraft-lua-') as temporary:
            scratch = Path(temporary)
            for relative in ['Logs', 'Scripts/DCS-BIOS/doc/json', 'Scripts/DCS-BIOS/doc/doc_assets']:
                (scratch / relative).mkdir(parents=True)
            configured = []
            for name in selected:
                data = (json.loads((ROOT / 'examples/dcs-bios/aircraft' / f'{name}.json').read_text())
                        if name != 'MH-6J' else {'name': name, 'base': 0xc000})
                mod = scratch / 'Mods/aircraft' / name
                mod.mkdir(parents=True)
                (mod / 'entry.lua').write_text('fixture')
                data['path'] = mod.as_posix()
                configured.append(data)
            # Verify a mod removed between setup and DCS launch is not advertised.
            if not selected:
                configured.append(dict(name='MH-6J', base=0xc000, path=(scratch / 'missing').as_posix(), missing=True))
            globals_ = dict(TEST_ROOT=scratch.as_posix(), BIOS_SOURCE=args.bios_source.resolve().as_posix(),
                            REPO=ROOT.as_posix(), CONFIG=json.dumps(configured))
            prelude = ''.join(f'{key}=[====[{value}]====]\n' for key, value in globals_.items())
            runner.run(prelude + (ROOT / 'tests/dcs-aircraft-exports.lua').read_text(), args.dll, args.lua)


if __name__ == '__main__':
    main()
