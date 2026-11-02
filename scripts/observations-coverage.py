"""Gate native observation delivery and preference producers at 71% production lines."""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
from native_coverage_sources import production_counts, production_code


def run(arguments, root, env=None):
    result=subprocess.run([str(value) for value in arguments],cwd=root,env=env,
                          text=True,encoding="utf-8",stdout=subprocess.PIPE,stderr=subprocess.STDOUT)
    if result.returncode:
        print(result.stdout,flush=True)
        result.check_returncode()
    return result.stdout


def main():
    root=Path(__file__).resolve().parents[1]
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output-dir",type=Path,default=root/".research/observations-coverage")
    output=parser.parse_args().output_dir.resolve();output.mkdir(parents=True,exist_ok=True)
    profiles=Path(tempfile.mkdtemp(prefix="profiles-",dir=output))
    env=dict(os.environ,CARGO_TARGET_DIR=os.environ.get("CARGO_TARGET_DIR",str(output/"target")),
             RUSTFLAGS="-C instrument-coverage",LLVM_PROFILE_FILE=str(profiles/"%m-%p.profraw"))
    print("Compiling instrumented native observation tests.",flush=True)
    artifacts=run(["cargo","test","--locked","--lib","--no-run","--message-format=json",
                   "--manifest-path",root/"src-tauri/Cargo.toml"],root,env)
    (output/"build.log").write_text(artifacts,encoding="utf-8")
    binaries=[item["executable"] for line in artifacts.splitlines() if line.startswith("{")
              and (item:=json.loads(line)).get("reason")=="compiler-artifact"
              and item.get("executable") and item.get("profile",{}).get("test")]
    if len(binaries)!=1: raise RuntimeError("Expected one native library test executable")
    logs=[]
    for selection in ["observations::","persistence::ui::"]:
        log=run([binaries[0],selection,"--test-threads=1"],root,env)
        print(log,flush=True);logs.append(log)
    (output/"tests.log").write_text("\n".join(logs),encoding="utf-8")
    sysroot=Path(run(["rustc","--print","sysroot"],root).strip())
    host=re.search(r"^host: (.+)$",run(["rustc","-vV"],root),re.M).group(1)
    suffix=".exe" if os.name=="nt" else ""
    llvm=sysroot/"lib/rustlib"/host/"bin";profile=output/"coverage.profdata"
    run([llvm/f"llvm-profdata{suffix}","merge","-sparse",*profiles.glob("*.profraw"),"-o",profile],root)
    sources=list((root/"src-tauri/src/observations").glob("*.rs"))
    sources.extend(root/f"src-tauri/src/persistence/ui/{name}" for name in ["mod.rs","streams.rs"])
    summary={}
    for source in sorted(sources):
        if source.stem=="tests":continue
        text=source.read_text(encoding="utf-8")
        annotated=run([llvm/f"llvm-cov{suffix}","show",binaries[0],f"-instr-profile={profile}",
                       "-show-line-counts-or-regions",source],root)
        name=str(source.relative_to(root)).replace("\\","/")
        (output/(name.replace("/","-")+"-lines.txt")).write_text(annotated,encoding="utf-8")
        counts=production_counts(text,annotated)
        if not counts:
            if re.search(r"\bfn\s+\w+",production_code(text)):raise RuntimeError(f"Missing coverage for {name}")
            continue
        covered=sum(counts.values());percent=round(100*covered/len(counts),2)
        summary[name]=dict(covered=covered,lines=len(counts),percent=percent)
        print(f"{name}: {percent}%",flush=True)
    (output/"summary.json").write_text(json.dumps(summary,indent=2)+"\n",encoding="utf-8")
    if not summary or any(item["percent"]<71 for item in summary.values()):
        raise SystemExit("Each observation producer/delivery module requires 71% production line coverage")


if __name__=="__main__":main()
