"""Stage a verified IDF board build for development embedding and release packaging."""
from pathlib import Path
import argparse, hashlib, json, re, shutil, subprocess


def stage_firmware(build, repository):
    cache=(build/"CMakeCache.txt").read_text()
    profiles={"waveshare-lcd7-s3":("esp32s3","0x0"),"sunton-3248s035r":("esp32","0x1000")}
    profile=next((name for name in profiles if "OSG_BOARD_PROFILE:STRING="+name in cache.splitlines()),None)
    if profile is None: raise SystemExit("Expected a supported physical board profile")
    chip,boot=profiles[profile]
    manifest=json.loads((build/"flasher_args.json").read_text())
    if manifest["extra_esptool_args"]["chip"]!=chip: raise SystemExit("Board profile and MCU do not match")
    root=repository/"src-tauri/resources/firmware"/profile
    header=(repository/"apps/companion-esp32/network/firmware.h").read_text()
    version=re.search(r'^#define OSG_FIRMWARE_VERSION "([^"\r\n]+)"$',header,re.MULTILINE).group(1)
    application=(build/manifest["flash_files"]["0x10000"]).resolve()
    if not application.is_relative_to(build.resolve()): raise SystemExit("Firmware file escapes its build directory")
    if b"\0"+version.encode("ascii")+b"\0" not in application.read_bytes():
        raise SystemExit("Application image does not contain the current firmware version. Rebuild this board before staging.")
    # Check every source before replacing any previously staged bundle.
    for source in manifest["flash_files"].values():
        source=(build/source).resolve()
        if not source.is_relative_to(build.resolve()): raise SystemExit("Firmware file escapes its build directory")
        if not source.is_file(): raise SystemExit("Firmware build is incomplete")
    root.mkdir(parents=True,exist_ok=True)
    records=[]
    for address,name in [(boot,"bootloader.bin"),("0x8000","partition-table.bin"),("0x10000","application.bin")]:
        source=(build/manifest["flash_files"][address]).resolve()
        if not source.is_relative_to(build.resolve()): raise SystemExit("Firmware file escapes its build directory")
        shutil.copyfile(source,root/name)
        records.append({"offset":address,"file":name,"sha256":hashlib.sha256(source.read_bytes()).hexdigest()})
    commit=subprocess.check_output(["git","rev-parse","HEAD"],cwd=repository,text=True).strip()
    (root/"manifest.json").write_text(json.dumps({"schemaVersion":1,"component":"esp32-companion","version":version,"board":profile,"chip":chip,"sourceCommit":commit,"segments":records},indent=2)+"\n",encoding="utf-8")
    print("Staged "+profile+" firmware for development and release packaging.")


if __name__ == "__main__":
    parser=argparse.ArgumentParser()
    parser.add_argument("build",type=Path)
    args=parser.parse_args()
    stage_firmware(args.build,Path(__file__).resolve().parents[1])
