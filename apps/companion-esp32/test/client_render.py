"""Actual client lifecycle with real streaming package engine and mocked SDK/host."""
from pathlib import Path
import gzip,json,os,subprocess
ROOT=Path(__file__).resolve().parents[3];APP=ROOT/'apps/companion-esp32';OUT=ROOT/'.research/esp32-client-render-native'
OUT.mkdir(parents=True,exist_ok=True)
flags=[os.environ.get('CC','gcc'),'-std=c11','-Wall','-Wextra','-Werror','-pedantic','-g','--coverage','-fsanitize=address,undefined','-DOSG_RENDER_ENGINE=1','-DCJSON_NESTING_LIMIT=24','-DCJSON_CIRCULAR_LIMIT=2048']
flags+=['-I'+str(APP/p) for p in ('test/include','test/content_store_include','core','render','network','third_party/cjson','test')]
sources=['network/render_delivery.c','network/render_feedback.c','render/engine.c','render/content_store.c','render/scene.c','render/package.c','render/expressions.c','core/companion.c','core/diagnostics.c','network/input.c','network/json.c','network/publication.c','network/settings.c','third_party/cjson/cJSON.c','test/client_render_test.c']
# Existing network.py covers all legacy paths. This separately gates newly compiled
# engine branches, including code inside the public touch/render hooks.
engine_lines=set();stack=[]
for index,line in enumerate((APP/'network/client.c').read_text().splitlines(),1):
    if line.startswith('#if'):
        stack.append(line.strip()=='#ifdef OSG_RENDER_ENGINE')
    elif line.startswith('#endif'):
        stack.pop()
    elif any(stack):engine_lines.add(index)
profiles={}
for board in ('sunton','waveshare'):
    run_root=OUT/board;run_root.mkdir(exist_ok=True)
    for path in run_root.glob('client-render-test-*.gcda'):path.unlink()
    profile_flags=['-DOSG_RENDER_SUNTON=1'] if board=='sunton' else []
    signature=json.dumps([flags,profile_flags,sources]);signature_path=run_root/'build-flags.json'
    rebuild=not signature_path.exists() or signature_path.read_text()!=signature
    header_time=max(path.stat().st_mtime_ns for folder in ('core','render','network','test/include','test/content_store_include') for path in (APP/folder).rglob('*.h'))
    header_time=max(header_time,(APP/'test/render_host_fixture.h').stat().st_mtime_ns)
    objects=[]
    for source in sources:
        path=APP/source;obj=run_root/('client-render-test-'+path.stem+'.o');objects.append(obj)
        modified=max(path.stat().st_mtime_ns,header_time)
        if source=='test/client_render_test.c':modified=max(modified,(APP/'network/client.c').stat().st_mtime_ns,(APP/'test/engine_test.c').stat().st_mtime_ns)
        if rebuild or not obj.exists() or obj.stat().st_mtime_ns<modified:
            subprocess.run([*flags,*profile_flags,'-c',str(path),'-o',str(obj)],check=True,cwd=ROOT)
    signature_path.write_text(signature)
    subprocess.run([flags[0],'-fsanitize=address,undefined','--coverage',*[str(p) for p in objects],'-lm','-lcrypto','-o',str(run_root/'client-render-test')],check=True,cwd=ROOT)
    result=subprocess.run([str(run_root/'client-render-test'),str(ROOT/'packages/companion-renderer/test/poc'),board,str(APP/'test/live-publication.json')],check=False,capture_output=True,text=True,cwd=ROOT)
    (run_root/'client-render-test.log').write_text(result.stdout+result.stderr);print(board+': '+result.stdout+result.stderr,end='');result.check_returncode()
    status_rows=[(line,json.loads(line)) for line in result.stdout.splitlines() if line.startswith('{')]
    maximum=next((line,row) for line,row in status_rows if row.get('requestId')=='r'*64)
    assert 1536<len(maximum[0].encode())<2048 and maximum[1]['name']=='\x01'*80 and maximum[1]['ssid']=='\x01'*32
    assert len(maximum[1]['serialHealth'])==13 and set(maximum[1]['serialHealth'].values())=={4294967295}
    subprocess.run(['gcov','-j','-b','-c','client-render-test-client_render_test.gcno'],cwd=run_root,check=True,stdout=subprocess.DEVNULL)
    data=json.loads(gzip.decompress((run_root/'client-render-test-client_render_test.gcov.json.gz').read_bytes()));module=next(f for f in data['files'] if f['file'].replace('/../','/').endswith('/network/client.c'))
    lines=[l for l in module['lines'] if l['line_number'] in engine_lines];branches=[b for l in lines for b in l.get('branches',[])];functions=[f for f in module['functions'] if f['start_line'] in engine_lines]
    metrics={'engineLines':100*sum(l['count']>0 for l in lines)/len(lines),'engineBranches':100*sum(b['count']>0 for b in branches)/len(branches),'engineFunctions':100*sum(f['execution_count']>0 for f in functions)/len(functions)}
    profiles[board]=metrics;print(board+': '+json.dumps(metrics))
    if any(v<71 for v in metrics.values()):raise SystemExit('Client render '+board+' coverage below71%')
(OUT/'coverage.json').write_text(json.dumps(profiles,indent=2)+'\n');print(json.dumps(profiles,indent=2))
