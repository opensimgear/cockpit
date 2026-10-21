"""Fetch the locked upstream archive into an ignored, checksum-verified cache."""
from hashlib import sha256
import json
from pathlib import Path, PurePosixPath
import tarfile
import urllib.request

APP = Path(__file__).resolve().parents[1]
ROOT = APP.parents[1]


def source():
    lock = json.loads((APP / 'third_party/lvgl.lock.json').read_text())
    cache = ROOT / '.research/lvgl-download'
    cache.mkdir(parents=True, exist_ok=True)
    archive = cache / 'source.tar.gz'
    if not archive.exists():
        request = urllib.request.Request(lock['url'], headers={'User-Agent': 'OpenSimGear-build'})
        with urllib.request.urlopen(request, timeout=60) as response:
            data = response.read(100 * 1024 * 1024 + 1)
        if len(data) > 100 * 1024 * 1024 or sha256(data).hexdigest() != lock['sha256']:
            raise RuntimeError('LVGL archive exceeds its budget or fails the locked digest')
        archive.write_bytes(data)
    if sha256(archive.read_bytes()).hexdigest() != lock['sha256']:
        raise RuntimeError('Cached LVGL archive fails the locked digest; remove that archive and retry')
    directory = cache / ('lvgl-' + lock['commit'])
    marker = directory / '.osg-verified-archive'
    if not marker.exists():
        with tarfile.open(archive, 'r:gz') as bundle:
            members = bundle.getmembers()
            for member in members:
                path = PurePosixPath(member.name)
                if path.is_absolute() or '..' in path.parts or not path.parts or path.parts[0] != directory.name or not (member.isfile() or member.isdir()):
                    raise RuntimeError('Unexpected path or link in locked LVGL archive')
            bundle.extractall(cache, members=members, filter='data')
        marker.write_text(lock['sha256'] + '\n')
    if marker.read_text().strip() != lock['sha256']:
        raise RuntimeError('LVGL cache marker does not match the locked source')
    return directory


if __name__ == '__main__':
    print(source())
