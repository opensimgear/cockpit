#!/usr/bin/env python3
"""Prepare a reviewed Windows-only qualification copy. Never build or run it.

Default: print the exact plan. --prepare writes a NEW private target only.
The ordinary desktop/service entry points and qualification flags are untouched.
"""
import argparse
import hashlib
import io
import json
from pathlib import Path, PurePosixPath
import re
import subprocess
import tarfile
import tempfile


ROOT_FILES = {
    'LICENSE', 'THIRD_PARTY_NOTICES.md', 'package.json', 'pnpm-lock.yaml',
    'pnpm-workspace.yaml', 'tsconfig.json', 'vite.config.ts',
    'svelte.config.js', 'rust-toolchain.toml',
}
PREFIXES = ('src-tauri/', 'crates/', 'packages/', 'apps/browser-companion/', 'scripts/')
FIRMWARE_HEADER = 'apps/companion-esp32/network/firmware.h'
EXTRA_BROWSER_SOURCES = {
    'apps/companion/src/lib/companion-cache.ts',
    'apps/companion/src/lib/input-queue.ts',
}
HARNESS = 'tests/manual/native-profile-qualification.rs'
MODULE = 'src-tauri/src/native_profile_qualification.rs'
OVERRIDES = (
    ('crates/mapping-host/src/service.rs',
     b'pub const LIVE_OUTPUT_QUALIFIED: bool = false;',
     b'pub const LIVE_OUTPUT_QUALIFIED: bool = true;'),
    ('crates/controller-input/src/visibility/service.rs',
     b'            qualified: false,', b'            qualified: true,'),
    ('crates/controller-input/src/visibility/windows.rs',
     b'pub const QUALIFIED: bool = false;', b'pub const QUALIFIED: bool = true;'),
)
MAIN = b'''// PRIVATE QUALIFICATION COPY: no desktop or GatewayManager fallback.
fn main() {
    if let Some(code) = osg_mapping_host::logic_helper_main() { std::process::exit(code); }
    if let Some(code) = osg_virtual_controller::entry::helper_main() { std::process::exit(code); }
    if let Some(code) = osg_virtual_controller::inspection::helper_main() { std::process::exit(code); }
    std::process::exit(osg_cockpit::native_profile_qualification::main());
}
'''
EMPTY = b'''
// PRIVATE QUALIFICATION COPY: the same empty OSG endpoint helper used by native tests.
#[cfg(not(test))]
impl ProfilePanelIo {
    pub(crate) fn empty() -> Self {
        Self::new(Arc::default(), Arc::new(crate::module_usb_fleet::Fleet::new(Arc::new(||Ok(vec![])),false)),
            Arc::default(),Arc::new(AtomicBool::new(false)))
    }
}
'''
TEST_EMPTY_ANCHOR = b'''#[cfg(test)]
impl ProfilePanelIo {
    #[cfg(test)]
    pub(crate) fn empty() -> Self {
        Self::new(Arc::default(), Arc::new(crate::module_usb_fleet::Fleet::new(Arc::new(||Ok(vec![])),false)),
            Arc::default(),Arc::new(AtomicBool::new(false)))
    }
'''


def sha(data):
    return hashlib.sha256(data).hexdigest()


def git(repository, *args):
    return subprocess.check_output(
        ['git', '-c', f'safe.directory={repository.as_posix()}', *args], cwd=repository)


def ordinary_path(path):
    """Reject symlinks/junctions in the existing part of a destination path."""
    for item in (path, *path.parents):
        try:
            info = item.lstat()
        except FileNotFoundError:
            continue
        if item.is_symlink() or getattr(info, 'st_file_attributes', 0) & 0x400:
            raise ValueError(f'Reparse-point destination is forbidden: {item}')


def checked_target(repository, target):
    target = target.absolute()
    ordinary_path(target)
    if target.exists():
        raise ValueError('Target already exists; qualification preparation never overwrites it.')
    allowed = (repository / '.research/native-profile-qualification').resolve()
    if not target.resolve().is_relative_to(allowed) or target.resolve() == allowed:
        raise ValueError(f'Target must be a new child of {allowed}')
    return target


def archive_sources(repository, commit):
    payload = git(repository, 'archive', '--format=tar', commit, '--',
                  'src-tauri', 'crates', 'packages', 'apps/browser-companion',
                  'scripts', FIRMWARE_HEADER, *sorted(EXTRA_BROWSER_SOURCES), *sorted(ROOT_FILES))
    files = {}
    with tarfile.open(fileobj=io.BytesIO(payload), mode='r:') as archive:
        for member in archive:
            if member.isdir():
                continue
            path = PurePosixPath(member.name)
            name = path.as_posix()
            if (not member.isfile() or path.is_absolute() or '..' in path.parts
                    or '.git' in path.parts
                    or not (name in ROOT_FILES or name in EXTRA_BROWSER_SOURCES or name == FIRMWARE_HEADER
                            or name.startswith(PREFIXES))):
                raise ValueError(f'Unexpected archived object: {name}')
            if any(part in {'node_modules', 'target', '.research', '.e2e'} for part in path.parts):
                raise ValueError(f'Generated or private data is forbidden in source copy: {name}')
            files[name] = archive.extractfile(member).read()
    for needed in ['src-tauri/Cargo.toml', 'src-tauri/Cargo.lock', 'src-tauri/build.rs',
                   'src-tauri/src/main.rs', 'src-tauri/src/lib.rs', FIRMWARE_HEADER,
                   'apps/browser-companion/vite.config.ts', *sorted(EXTRA_BROWSER_SOURCES)]:
        if needed not in files:
            raise ValueError(f'Reviewed commit is missing required build input: {needed}')
    return files


def patch_once(files, path, before, after):
    if path not in files or files[path].count(before) != 1:
        raise ValueError(f'Unknown or ambiguous qualification patch anchor: {path}')
    files[path] = files[path].replace(before, after, 1)


def reviewed_harness(repository, harness_hash):
    source = (repository / HARNESS).read_bytes()
    if not re.fullmatch(r'[0-9a-f]{64}', harness_hash) or sha(source) != harness_hash:
        raise ValueError('The manual harness differs from its explicitly reviewed SHA-256.')
    return source


def prepare_plan(repository, commit, target, harness_hash):
    if not re.fullmatch(r'[0-9a-f]{40}', commit):
        raise ValueError('Use the full reviewed commit SHA, never a moving branch name.')
    if git(repository, 'rev-parse', f'{commit}^{{commit}}').decode().strip() != commit:
        raise ValueError('The reviewed commit could not be resolved exactly.')
    files = archive_sources(repository, commit)
    original = {path: sha(data) for path, data in files.items()}
    source = reviewed_harness(repository, harness_hash)
    for path, before, after in OVERRIDES:
        patch_once(files, path, before, after)
    lib = 'src-tauri/src/lib.rs'
    if b'mod native_profile_qualification' in files[lib]:
        raise ValueError('The reviewed library already contains a qualification entry.')
    files[lib] += b'\n// PRIVATE QUALIFICATION COPY ONLY.\npub mod native_profile_qualification;\n'
    main = 'src-tauri/src/main.rs'
    if files[main].count(b'osg_cockpit::run();') != 1:
        raise ValueError('Unknown desktop main entry; refuse to guess a safe replacement.')
    files[main] = MAIN
    panel = 'src-tauri/src/profile_session_panels_io.rs'
    # Recognize both checked-in line endings without rewriting the original
    # production bytes or weakening the exact helper-body anchor.
    if files[panel].replace(b'\r\n', b'\n').count(TEST_EMPTY_ANCHOR) != 1:
        raise ValueError('Unknown ProfilePanelIo::empty test-helper anchor.')
    files[panel] += EMPTY
    files[MODULE] = source
    allowed = {path for path, _, _ in OVERRIDES} | {lib, main, panel}
    changed = [path for path in original if sha(files[path]) != original[path]]
    if set(changed) != allowed:
        raise ValueError(f'Unexpected private-copy production delta: {changed}')
    stamp = {
        'schemaVersion': 1, 'scope': 'private Windows qualification copy; not a qualified release',
        'reviewedCommit': commit, 'manualHarnessSha256': harness_hash,
        'privateCopyRoot': str(target),
        'originalSources': original,
        'qualificationOverrides': [
            {'path': p, 'before': a.decode(), 'after': b.decode()} for p, a, b in OVERRIDES],
        'otherAllowedDeltas': [lib, main, panel, MODULE],
        'privateSources': {path: sha(data) for path, data in files.items()},
        'unchangedProductionSources': sorted(set(original) - allowed),
        'noGatewayManager': True, 'noBuildOrRunPerformed': True,
    }
    files['src-tauri/qualification-copy.json'] = (json.dumps(stamp, indent=2) + '\n').encode()
    plan = {
        'target': str(target), 'reviewedCommit': commit, 'sourceFileCount': len(files),
        'qualificationOverrides': stamp['qualificationOverrides'],
        'otherAllowedDeltas': stamp['otherAllowedDeltas'],
        'dependencyPreparation': 'Review reuse of existing pnpm dependencies with an exact lockfile match first. Ancestor workspace-package links must resolve to source identical to this pinned manifest, or be relinked in the private copy. Install --frozen-lockfile only if needed and separately authorized.',
        'productionDeltaManifest': [
            {'path': path, 'reviewedSha256': original[path], 'privateSha256': sha(files[path])}
            for path in sorted(changed)],
        'unchangedReviewedFileCount': len(original) - len(changed),
        'reviewedSourceManifestSha256': sha(json.dumps(original, sort_keys=True).encode()),
        'commandsForLaterReviewOnly': [
            {'cwd': str(target), 'args': ['pnpm', 'browser:build']},
            {'cwd': str(target), 'args': ['cargo', 'build', '--locked', '--manifest-path',
             'src-tauri/Cargo.toml', '--bin', 'osg-cockpit', '--target-dir', str(target / '.target')]},
        ],
        'execution': 'NOT AUTHORIZED BY THIS TOOL. Review source/copy manifest before any build or hardware use.',
        'firstFutureMode': '--native-profile-qualification preflight <absolute-private-config.json>',
        'configurationPreparation': 'Create a new ordinary private-state child directory and save the reviewed config there. Empty selections permit read-only discovery; no Apply is permitted until exact native IDs, all selected ports and the complete graph are reviewed.',
        'activation': 'Explicit apply only after real ID/port capture, reviewed profile, and guardian arm.',
        'configTemplate': {'schemaVersion': 1, 'reviewedCommit': commit,
                           'privateRoot': str(target / 'private-state'),
                           'vkb': [], 'pedals': [], 'requiredBindings': []},
        'scope': 'Actual MH-6J DCS + reviewed current VKB flight/view bindings + hidden-only pedals + actual browser WSS; OSG hardware deferred.',
    }
    return files, stamp, plan


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--self-check', action='store_true', help='Pure/disposable helper checks only; never copy, build, run, or inspect hardware.')
    parser.add_argument('--repository', type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument('--reviewed-commit')
    parser.add_argument('--harness-sha256')
    parser.add_argument('--target', type=Path)
    parser.add_argument('--prepare', action='store_true', help='Write a new private copy; never build or run.')
    args = parser.parse_args()
    if args.self_check:
        if args.prepare or args.reviewed_commit or args.harness_sha256 or args.target:
            parser.error('--self-check cannot be combined with copy inputs')
        self_check()
        return
    if not args.reviewed_commit or not args.harness_sha256 or args.target is None:
        parser.error('--reviewed-commit, --harness-sha256 and --target are required')
    repository = args.repository.resolve(strict=True)
    target = checked_target(repository, args.target)
    files, stamp, plan = prepare_plan(repository, args.reviewed_commit, target, args.harness_sha256)
    print(json.dumps(plan, indent=2), flush=True)
    if not args.prepare:
        return
    target.mkdir(parents=True, exist_ok=False)
    for name, data in files.items():
        destination = target.joinpath(*PurePosixPath(name).parts)
        if not destination.resolve().is_relative_to(target.resolve()):
            raise ValueError(f'Copy escaped target: {name}')
        destination.parent.mkdir(parents=True, exist_ok=True)
        ordinary_path(destination)
        with destination.open('xb') as output:
            output.write(data)
    for name, expected in stamp['privateSources'].items():
        if sha((target / name).read_bytes()) != expected:
            raise ValueError(f'Private source changed during preparation: {name}')
    with (target / 'qualification-build-plan.json').open('x', encoding='utf8') as output:
        output.write(json.dumps(plan, indent=2) + '\n')
    print('Prepared private copy only. No dependencies installed, build executed, driver opened, or hardware acquired.')


def self_check():
    checks = []

    def rejected(call):
        try:
            call()
        except ValueError:
            return
        raise AssertionError('Expected a closed failure')

    assert sha(b'abc') == 'ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad'
    checks.append('sha256')
    files = {'one': b'false'}
    patch_once(files, 'one', b'false', b'true')
    assert files['one'] == b'true'
    rejected(lambda: patch_once(files, 'absent', b'false', b'true'))
    files['one'] = b'false false'
    rejected(lambda: patch_once(files, 'one', b'false', b'true'))
    checks.extend(['exact patch', 'missing anchor rejects', 'ambiguous anchor rejects'])
    with tempfile.TemporaryDirectory(prefix='osg-qualification-preparer-') as temporary:
        repository = Path(temporary).resolve()
        allowed = repository / '.research/native-profile-qualification/new-copy'
        assert checked_target(repository, allowed) == allowed
        rejected(lambda: checked_target(repository, repository / 'escape'))
        existing = repository / '.research/native-profile-qualification/existing'
        existing.mkdir(parents=True)
        rejected(lambda: checked_target(repository, existing))
        checks.extend(['new contained target', 'escape rejects', 'existing target rejects'])
        harness = repository / HARNESS
        harness.parent.mkdir(parents=True)
        harness.write_bytes(b'// reviewed manual source\n')
        assert reviewed_harness(repository, sha(harness.read_bytes())) == harness.read_bytes()
        rejected(lambda: reviewed_harness(repository, '0' * 64))
        checks.extend(['reviewed harness accepted', 'mismatched harness hash rejects'])
        link = repository / 'broken'
        try:
            link.symlink_to(repository / 'missing', target_is_directory=True)
        except (OSError, NotImplementedError):
            checks.append('broken-link OS check unavailable; lstat/reparse guard retained')
        else:
            rejected(lambda: ordinary_path(link / 'child'))
            checks.append('broken-link ancestor rejects')
    print(json.dumps({'selfCheck': 'PASS', 'checks': checks,
                      'builds': 0, 'harnessExecutions': 0, 'hardwareOperations': 0}, indent=2))


if __name__ == '__main__':
    main()
