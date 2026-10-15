#!/usr/bin/env python3
"""Measure all logic modules across evaluator tests and real helper processes."""
import argparse,json,os,re,subprocess,tempfile
from pathlib import Path

def run(args,root,env=None):
    result=subprocess.run([str(a) for a in args],cwd=root,env=env,text=True,encoding='utf-8',stdout=subprocess.PIPE)
    if result.returncode:
        print(result.stdout)
        raise SystemExit(result.returncode)
    return result.stdout

def main():
    root=Path(__file__).resolve().parents[1]
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output-dir',type=Path,default=root/'.research/logic-coverage')
    output=parser.parse_args().output_dir.resolve();output.mkdir(parents=True,exist_ok=True)
    profiles=Path(tempfile.mkdtemp(prefix='profiles-',dir=output))
    env=dict(os.environ,CARGO_TARGET_DIR=str(output/'target'),RUSTFLAGS='-C instrument-coverage',LLVM_PROFILE_FILE=str(profiles/'%m-%p.profraw'))
    logs=run(['cargo','test','--locked','--message-format=json','--manifest-path',root/'crates/logic/Cargo.toml'],root,env)
    (output/'tests.log').write_text(logs,encoding='utf-8')
    entries=[]
    for line in logs.splitlines():
        if not line.startswith('{'): continue
        entry=json.loads(line)
        if entry.get('reason')=='compiler-artifact' and entry.get('executable'): entries.append(entry['executable'])
    binaries=list(dict.fromkeys(entries))
    if not binaries: raise RuntimeError('No instrumented logic binaries.')
    sysroot=Path(run(['rustc','--print','sysroot'],root).strip())
    host=re.search(r'^host: (.+)$',run(['rustc','-vV'],root),re.M).group(1)
    suffix='.exe' if os.name=='nt' else ''
    llvm=sysroot/'lib/rustlib'/host/'bin';profile=output/'coverage.profdata'
    run([llvm/f'llvm-profdata{suffix}','merge','-sparse',*profiles.glob('*.profraw'),'-o',profile],root)
    objects=[item for binary in binaries[1:] for item in ['-object',binary]]
    summary={}
    for module in ['evaluator','wire','worker','resources','replay','main']:
        source=root/f'crates/logic/src/{module}.rs'
        annotated=run([llvm/f'llvm-cov{suffix}','show',binaries[0],*objects,f'-instr-profile={profile}','-show-line-counts-or-regions',source],root)
        (output/f'{module}-lines.txt').write_text(annotated,encoding='utf-8')
        lines=source.read_text(encoding='utf-8').splitlines()
        end=next((i+1 for i,line in enumerate(lines) if line.strip()=='#[cfg(test)]'),len(lines)+1)
        counts={int(m[1]):int(m[2]) for line in annotated.splitlines() if (m:=re.match(r'^\s*(\d+)\|\s*(\d+)\|',line)) and int(m[1])<end}
        if not counts: raise RuntimeError(f'No production coverage for {module}.')
        covered=sum(n>0 for n in counts.values());percent=100*covered/len(counts)
        summary[module]=dict(covered=covered,lines=len(counts),percent=round(percent,2))
        print(f'{module}: {percent:.2f}%',flush=True)
    (output/'summary.json').write_text(json.dumps(summary,indent=2)+'\n',encoding='utf-8')
    if any(s['percent']<71 for s in summary.values()):raise SystemExit('Every logic module requires 71% production line coverage.')
if __name__=='__main__':main()
