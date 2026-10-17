import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('mobile_build', Path(__file__).with_name('companion-mobile-build.py'))
mobile_build = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mobile_build)


class MobileProjects(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)

    def gradle(self, sdk):
        path = self.root / 'build.gradle.kts'
        path.write_text('android {\n    compileSdk = ' + str(sdk) + '\n    defaultConfig {\n        minSdk = 29\n        targetSdk = ' + str(sdk) + '\n    }\n}\n', encoding='utf-8')
        return path

    def test_minor_zero_platform_and_gradle_select_same_api(self):
        gradle = self.gradle(37)
        self.assertEqual(mobile_build.android_platform(gradle), 'platforms;android-37.0')
        source = gradle.read_text(encoding='utf-8')
        self.assertIn('version = release(37) {\n            minorApiLevel = 0', source)
        self.assertIn('targetSdk = 37', source)
        self.assertIn('minSdk = 29', source)

    def test_older_sdk_keeps_existing_package_name_and_project(self):
        gradle = self.gradle(36)
        before = gradle.read_bytes()
        self.assertEqual(mobile_build.android_platform(gradle), 'platforms;android-36')
        self.assertEqual(gradle.read_bytes(), before)

    def test_changed_or_ambiguous_generator_format_fails_without_writing(self):
        for content in ['android {}', 'compileSdk = 37\ncompileSdk = 38\n', 'compileSdk = unknown', 'compileSdk = 999']:
            gradle = self.root / 'build.gradle.kts'
            gradle.write_text(content, encoding='utf-8')
            with self.assertRaises(ValueError):
                mobile_build.android_platform(gradle)
            self.assertEqual(gradle.read_text(encoding='utf-8'), content)

    def project(self, hook='pnpm tauri ios xcode-script'):
        project = self.root / 'gen/apple'
        pbx = project / 'companion.xcodeproj/project.pbxproj'
        pbx.parent.mkdir(parents=True)
        arguments = ' -v --platform "${PLATFORM_DISPLAY_NAME:?}" --sdk-root "${SDKROOT:?}" ${ARCHS:?}\n'
        script = hook + arguments
        pbx.write_text('shellScript = ' + json.dumps(script) + ';\n', encoding='utf-8')
        (project / 'project.yml').write_text('preBuildScripts:\n  - script: |\n      ' + script + '    name: Build Rust Code\n', encoding='utf-8')
        node = self.root / 'Node with spaces/node'
        cli = self.root / 'App with spaces/node_modules/@tauri-apps/cli/tauri.js'
        node.parent.mkdir(); node.touch()
        cli.parent.mkdir(parents=True); cli.touch()
        return project, pbx, node, cli, arguments

    def test_nested_xcode_hook_uses_installed_node_cli_and_preserves_arguments(self):
        project, pbx, node, cli, arguments = self.project()
        mobile_build.ios_rust_hook(project, node, cli)
        script = json.loads(pbx.read_text(encoding='utf-8').removeprefix('shellScript = ').strip().removesuffix(';'))
        self.assertNotIn('pnpm', script)
        self.assertIn(str(node.resolve()), script)
        self.assertIn(str(cli.resolve()), script)
        self.assertIn('env -u npm_lifecycle_event -u PNPM_PACKAGE_NAME', script)
        self.assertTrue(script.endswith(arguments))
        self.assertIn('ios xcode-script', script)
        self.assertNotIn('pnpm tauri', (project / 'project.yml').read_text(encoding='utf-8'))

    def test_unrecognized_hook_preserves_generated_files(self):
        project, pbx, node, cli, _ = self.project()
        pbx.write_text('shellScript = "different hook";\n', encoding='utf-8')
        before_pbx, before_yml = pbx.read_bytes(), (project / 'project.yml').read_bytes()
        with self.assertRaisesRegex(ValueError, 'Xcode project'):
            mobile_build.ios_rust_hook(project, node, cli)
        self.assertEqual(pbx.read_bytes(), before_pbx)
        self.assertEqual((project / 'project.yml').read_bytes(), before_yml)


if __name__ == '__main__':
    unittest.main()
