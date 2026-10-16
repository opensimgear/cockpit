"""Exercise the actual app_main scene pump with deterministic platform boundaries."""
from pathlib import Path
import gzip,json,os,subprocess
ROOT=Path(__file__).resolve().parents[3];APP=ROOT/"apps/companion-esp32";OUT=ROOT/".research/esp32-main-scene-native"
OUT.mkdir(parents=True,exist_ok=True)
for p in OUT.glob("main-test-*.gcda"):p.unlink()
flags=[os.environ.get("CC","gcc"),"-std=c11","-Wall","-Wextra","-Werror","-pedantic","-g","--coverage","-fsanitize=address,undefined","-ffunction-sections","-fdata-sections"]
flags += ["-I"+str(APP/p) for p in ("test/main_include","test/include","test","core","boards","network","render","third_party/cjson")]
subprocess.run([*flags,str(APP/"test/main_scene_test.c"),"-Wl,--gc-sections","-o",str(OUT/"main-test")],check=True)
subprocess.run([str(OUT/"main-test")],check=True)
subprocess.run(["gcov","-j","-b","-c","main-test-main_scene_test.gcno"],check=True,cwd=OUT,stdout=subprocess.DEVNULL)
data=json.loads(gzip.decompress((OUT/"main-test-main_scene_test.gcov.json.gz").read_bytes()));f=next(f for f in data["files"] if f["file"].endswith("/main/app_main.c"))
# The separately tested firmware startup/legacy/touch loops are discarded by the
# linker; gate only the newly added scene_tick function in this boundary test.
fn=next(fn for fn in f["functions"] if fn["name"]=="scene_tick")
lines=[line for line in f["lines"] if fn["start_line"]<=line["line_number"]<=fn["end_line"]]
branches=[b for line in lines for b in line.get("branches",[])]
metrics={"lines":100*sum(l["count"]>0 for l in lines)/len(lines),"branches":100*sum(b["count"]>0 for b in branches)/len(branches),"functions":100 if fn["execution_count"] else 0}
(OUT/"coverage.json").write_text(json.dumps(metrics,indent=2)+"\n");print(json.dumps(metrics,indent=2))
if any(v<71 for v in metrics.values()):raise SystemExit("Scene-main coverage below71%")