"""Test the real storage adapter against a deterministic NVS double."""
from pathlib import Path
import subprocess
root=Path(__file__).resolve().parents[3]
app=root/"apps/companion-esp32"
out=root/".research/esp32-storage"
out.mkdir(parents=True,exist_ok=True)
subprocess.run(["gcc","-std=c11","-Wall","-Wextra","-Werror","-pedantic","-g","-fsanitize=address,undefined",
 *["-I"+str(app/p) for p in ["test/include","network","core","third_party/cjson"]],
 *[str(app/p) for p in ["test/storage_test.c","network/settings.c","network/json.c", "network/workspace.c","third_party/cjson/cJSON.c"]],
 "-lm","-o",str(out/"storage-test")],check=True)
subprocess.run([str(out/"storage-test")],check=True)
