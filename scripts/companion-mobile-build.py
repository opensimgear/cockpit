#!/usr/bin/env python3
"""Configure generated mobile projects without changing their runtime targets."""
import json
from pathlib import Path
import re
import shlex
import shutil
import sys

ROOT = Path(__file__).resolve().parents[1]
APP = ROOT / 'apps/companion'


def android_platform(gradle):
    source = gradle.read_text(encoding='utf-8')
    assignment = re.compile(r'(?m)^(?P<indent>[ \t]*)compileSdk[ \t]*=[ \t]*(?P<major>[0-9]+)[ \t]*$')
    matches = list(assignment.finditer(source))
    if len(matches) != 1:
        raise ValueError('Expected one generated integer compileSdk declaration')
    match = matches[0]
    major = int(match['major'])
    if not 21 <= major <= 99:
        raise ValueError('Unsupported generated Android API level')
    if major < 37:
        return f'platforms;android-{major}'
    # Google publishes API 37 onward with a minor suffix, including minor zero.
    # Use AGP's explicit DSL so Gradle and sdkmanager select the same platform.
    # https://developer.android.com/build#android-sdk-settings
    indent = match['indent']
    replacement = '\n'.join([
        f'{indent}compileSdk {{', f'{indent}    version = release({major}) {{',
        f'{indent}        minorApiLevel = 0', f'{indent}    }}', f'{indent}}}',
    ])
    gradle.write_text(source[:match.start()] + replacement + source[match.end():], encoding='utf-8')
    return f'platforms;android-{major}.0'


def ios_rust_hook(project, node, cli):
    if not node.is_file() or not cli.is_file():
        raise ValueError('The installed Node and Tauri CLI entry point are required')
    projects = list(project.glob('*.xcodeproj/project.pbxproj'))
    if len(projects) != 1:
        raise ValueError('Expected one generated Xcode project')
    # Tauri's native hook recognizes gen/apple from the absence of lifecycle
    # variables. Do not inherit the outer pnpm command's package-script context.
    command = ('env -u npm_lifecycle_event -u PNPM_PACKAGE_NAME -u npm_execpath -u npm_config_user_agent '
               f'{shlex.quote(str(node.resolve()))} {shlex.quote(str(cli.resolve()))} ios xcode-script')
    original = 'pnpm tauri ios xcode-script'
    yml = project / 'project.yml'
    yml_source = yml.read_text(encoding='utf-8')
    if yml_source.count(original) != 1:
        raise ValueError('Expected one pnpm Rust hook in generated project.yml')
    pbx = projects[0]
    pbx_source = pbx.read_text(encoding='utf-8')
    scripts = re.compile(r'(shellScript\s*=\s*)("(?:\\.|[^"\\])*")(\s*;)')
    changed = 0

    def replace_script(match):
        nonlocal changed
        script = json.loads(match[2])
        if original not in script:
            return match[0]
        if script.count(original) != 1:
            raise ValueError('Ambiguous generated Xcode Rust hook')
        changed += 1
        return match[1] + json.dumps(script.replace(original, command), ensure_ascii=False) + match[3]

    updated = scripts.sub(replace_script, pbx_source)
    if changed != 1:
        raise ValueError('Expected one pnpm Rust hook in generated Xcode project')
    # Validate both before writing either. Keep the template consistent if the
    # project is regenerated, and preserve every platform/SDK/architecture flag.
    yml.write_text(yml_source.replace(original, command), encoding='utf-8')
    pbx.write_text(updated, encoding='utf-8')


def main(command):
    if command == 'android-platform':
        print(android_platform(APP / 'src-tauri/gen/android/app/build.gradle.kts'))
    elif command == 'ios-rust-hook':
        node = shutil.which('node')
        if not node:
            raise ValueError('Node is not installed')
        ios_rust_hook(APP / 'src-tauri/gen/apple', Path(node), APP / 'node_modules/@tauri-apps/cli/tauri.js')
    else:
        raise ValueError(f'Unknown mobile project configuration: {command}')


if __name__ == '__main__':
    main(*sys.argv[1:])
