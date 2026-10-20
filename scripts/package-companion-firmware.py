"""Stage a verified IDF board build for development embedding and release packaging."""
from pathlib import Path
import argparse, hashlib, json, re, shutil, subprocess
parser=argparse.ArgumentParser()
parser.add_argument("build",type=Path)
args=parser.parse_args()
cache=(args.build/"CMakeCache.txt").read_text()
profiles={"waveshare-lcd7-s3":("esp32s3","0x0"),"sunton-3248s035r":("esp32","0x1000")}
profile=next((name for name in profiles if "OSG_BOARD_PROFILE:STRING="+name in cache.splitlines()),None)
if profile is None: raise SystemExit("Expected a supported physical board profile")
chip,boot=profiles[profile]
manifest=json.loads((args.build/"flasher_args.json").read_text())
if manifest["extra_esptool_args"]["chip"]!=chip: raise SystemExit("Board profile and MCU do not match")
root=Path(__file__).resolve().parents[1]/"src-tauri/resources/firmware"/profile
root.mkdir(parents=True,exist_ok=True)
records=[]
for address,name in [(boot,"bootloader.bin"),("0x8000","partition-table.bin"),("0x10000","application.bin")]:
    source=(args.build/manifest["flash_files"][address]).resolve()
    if not source.is_relative_to(args.build.resolve()): raise SystemExit("Firmware file escapes its build directory")
    shutil.copyfile(source,root/name)
    records.append({"offset":address,"file":name,"sha256":hashlib.sha256(source.read_bytes()).hexdigest()})
repository=Path(__file__).resolve().parents[1]
header=(repository/"apps/companion-esp32/network/firmware.h").read_text()
version=re.search(r'^#define OSG_FIRMWARE_VERSION "([^"\r\n]+)"$',header,re.MULTILINE).group(1)
commit=subprocess.check_output(["git","rev-parse","HEAD"],cwd=repository,text=True).strip()
(root/"manifest.json").write_text(json.dumps({"schemaVersion":1,"component":"esp32-companion","version":version,"board":profile,"chip":chip,"sourceCommit":commit,"segments":records},indent=2)+"\n",encoding="utf-8")
print("Staged "+profile+" firmware for development and release packaging.")
