"""Board-specific engine builds: real PoCs, raw flash/SHA, C scene and sessions."""
from pathlib import Path
import gzip,json,os,subprocess
ROOT=Path(__file__).resolve().parents[3];APP=ROOT/"apps/companion-esp32";OUT=ROOT/".research/esp32-engine-native"
OUT.mkdir(parents=True,exist_ok=True)
base=[os.environ.get("CC","gcc"),"-std=c11","-Wall","-Wextra","-Werror","-pedantic","-g","--coverage","-fsanitize=address,undefined","-DCJSON_NESTING_LIMIT=24","-DCJSON_CIRCULAR_LIMIT=2048"]
base += ["-I"+str(APP/p) for p in ("core","render","network","third_party/cjson")]
sources=["render/engine.c","render/content_store.c","render/scene.c","render/package.c","render/expressions.c","core/companion.c","network/json.c","third_party/cjson/cJSON.c","test/engine_test.c"]
covered_lines={};covered_branches={};covered_functions={}
for board,name in enumerate(("sunton","waveshare")):
    directory=OUT/name;directory.mkdir(exist_ok=True)
    for counter in directory.glob("engine-test-*.gcda"):counter.unlink()
    flags=base+(["-DOSG_RENDER_SUNTON=1"] if board==0 else [])
    subprocess.run([*flags,*[str(APP/p) for p in sources],"-lm","-lcrypto","-o",str(directory/"engine-test")],check=True)
    result=subprocess.run([str(directory/"engine-test"),str(ROOT/"packages/companion-renderer/test/poc"),str(board)],check=False,capture_output=True,text=True)
    (directory/"engine-test.log").write_text(result.stdout+result.stderr);print(name+": "+result.stdout+result.stderr,end="");result.check_returncode()
    subprocess.run(["gcov","-j","-b","-c","engine-test-engine.gcno"],check=True,cwd=directory,stdout=subprocess.DEVNULL)
    data=json.loads(gzip.decompress((directory/"engine-test-engine.gcov.json.gz").read_bytes()));source=next(f for f in data["files"] if f["file"].endswith("/render/engine.c"))
    for line in source["lines"]:
        key=line["line_number"];covered_lines[key]=covered_lines.get(key,False) or line["count"]>0
        for i,branch in enumerate(line.get("branches",[])):
            k=(key,i);covered_branches[k]=covered_branches.get(k,False) or branch["count"]>0
    for f in source["functions"]:covered_functions[f["name"]]=covered_functions.get(f["name"],False) or f["execution_count"]>0
metrics={name:100*sum(values.values())/len(values) for name,values in (("lines",covered_lines),("branches",covered_branches),("functions",covered_functions))}
(OUT/"coverage.json").write_text(json.dumps(metrics,indent=2)+"\n");print(json.dumps(metrics,indent=2))
if any(v<71 for v in metrics.values()):raise SystemExit("Engine coverage below71%")