import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

spec=importlib.util.spec_from_file_location("package_firmware",Path(__file__).with_name("package-companion-firmware.py"))
package=importlib.util.module_from_spec(spec)
spec.loader.exec_module(package)

class PackageFirmwareTests(unittest.TestCase):
    def fixture(self,root,version="2026.10.21.1"):
        build=root/"build";build.mkdir()
        (build/"CMakeCache.txt").write_text("OSG_BOARD_PROFILE:STRING=sunton-3248s035r\n")
        (build/"flasher_args.json").write_text(json.dumps({"extra_esptool_args":{"chip":"esp32"},"flash_files":{"0x1000":"boot.bin","0x8000":"part.bin","0x10000":"app.bin"}}))
        for name in ["boot.bin","part.bin"]:(build/name).write_bytes(b"segment")
        (build/"app.bin").write_bytes(b"app\0"+version.encode()+b"\0")
        header=root/"apps/companion-esp32/network/firmware.h";header.parent.mkdir(parents=True)
        header.write_text('#define OSG_FIRMWARE_VERSION "2026.10.22.3"\n')
        return build
    def test_rejects_stale_image_before_changing_staged_files(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);build=self.fixture(root)
            staged=root/"src-tauri/resources/firmware/sunton-3248s035r";staged.mkdir(parents=True);(staged/"application.bin").write_bytes(b"previous")
            with self.assertRaisesRegex(SystemExit,"Rebuild this board"):
                package.stage_firmware(build,root)
            self.assertEqual((staged/"application.bin").read_bytes(),b"previous")
            self.assertFalse((staged/"manifest.json").exists())
    def test_stages_matching_image_and_its_manifest(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);build=self.fixture(root,"2026.10.22.3")
            with patch.object(package.subprocess,"check_output",return_value="a"*40):
                package.stage_firmware(build,root)
            staged=root/"src-tauri/resources/firmware/sunton-3248s035r"
            manifest=json.loads((staged/"manifest.json").read_text())
            self.assertEqual(manifest["version"],"2026.10.22.3")
            self.assertEqual((staged/"application.bin").read_bytes(),(build/"app.bin").read_bytes())
            self.assertEqual(len(manifest["segments"]),3)

if __name__=="__main__":unittest.main()
