"""Native management delivery state machine with real engine/store and mock host."""
from pathlib import Path
import gzip,json,os,subprocess
ROOT=Path(__file__).resolve().parents[3];APP=ROOT/'apps/companion-esp32';OUT=ROOT/'.research/esp32-render-delivery-native'
OUT.mkdir(parents=True,exist_ok=True)
flags=[os.environ.get('CC','gcc'),'-std=c11','-Wall','-Wextra','-Werror','-pedantic','-g','--coverage','-fsanitize=address,undefined','-DCJSON_NESTING_LIMIT=24','-DCJSON_CIRCULAR_LIMIT=2048']
flags+=['-I'+str(APP/p) for p in ('core','render','network','third_party/cjson')]
sources=['network/render_delivery.c','render/engine.c','render/content_store.c','render/scene.c','render/package.c','render/expressions.c','core/companion.c','network/json.c','third_party/cjson/cJSON.c','test/render_delivery_test.c']
profiles={}
for board in ('sunton','waveshare'):
    run_root=OUT/board;run_root.mkdir(exist_ok=True)
    for path in run_root.glob('delivery-test-*.gcda'):path.unlink()
    profile_flags=['-DOSG_RENDER_SUNTON=1'] if board=='sunton' else []
    subprocess.run([*flags,*profile_flags,*[str(APP/p) for p in sources],'-lm','-lcrypto','-o',str(run_root/'delivery-test')],check=True,cwd=ROOT)
    result=subprocess.run([str(run_root/'delivery-test'),str(ROOT/'packages/companion-renderer/test/poc'),board],check=False,capture_output=True,text=True,cwd=ROOT)
    (run_root/'delivery-test.log').write_text(result.stdout+result.stderr);print(board+': '+result.stdout+result.stderr,end='');result.check_returncode()
    subprocess.run(['gcov','-j','-b','-c','delivery-test-render_delivery.gcno'],cwd=run_root,check=True,stdout=subprocess.DEVNULL)
    data=json.loads(gzip.decompress((run_root/'delivery-test-render_delivery.gcov.json.gz').read_bytes()));module=next(f for f in data['files'] if f['file'].endswith('/network/render_delivery.c'))
    lines=module['lines'];branches=[b for l in lines for b in l.get('branches',[])];functions=module['functions']
    metrics={'lines':100*sum(l['count']>0 for l in lines)/len(lines),'branches':100*sum(b['count']>0 for b in branches)/len(branches),'functions':100*sum(f['execution_count']>0 for f in functions)/len(functions)}
    profiles[board]=metrics
    if any(v<71 for v in metrics.values()):raise SystemExit('Render delivery '+board+' coverage below71%')
(OUT/'coverage.json').write_text(json.dumps(profiles,indent=2)+'\n');print(json.dumps(profiles,indent=2))
