"""Production serial reply parser with bounded, redacted failure counters."""
from pathlib import Path
import gzip,json,os,subprocess
ROOT=Path(__file__).resolve().parents[3];APP=ROOT/'apps/companion-esp32';OUT=ROOT/'.research/esp32-serial-wire'
OUT.mkdir(parents=True,exist_ok=True)
for p in OUT.glob('serial-wire-test-*.gcda'):p.unlink()
flags=[os.environ.get('CC','gcc'),'-std=c11','-Wall','-Wextra','-Werror','-pedantic','-g','--coverage','-fsanitize=address,undefined','-DCONFIG_ESP_CONSOLE_UART=0','-DCJSON_NESTING_LIMIT=24','-DCJSON_CIRCULAR_LIMIT=2048']
flags+=['-I'+str(APP/p) for p in ('test/serial_include','test/include','core','network','third_party/cjson')]
sources=['network/serial.c','network/json.c','core/diagnostics.c','third_party/cjson/cJSON.c','test/serial_wire_test.c']
subprocess.run([*flags,*[str(APP/p) for p in sources],'-lm','-o',str(OUT/'serial-wire-test')],check=True,cwd=ROOT)
subprocess.run([str(OUT/'serial-wire-test')],check=True,cwd=ROOT)
subprocess.run(['gcov','-j','-b','-c','serial-wire-test-serial_wire_test.gcno'],check=True,cwd=OUT,stdout=subprocess.DEVNULL)
data=json.loads(gzip.decompress((OUT/'serial-wire-test-serial_wire_test.gcov.json.gz').read_bytes()));source=next(f for f in data['files'] if f['file'].replace('/../','/').endswith('/network/serial_wire.c'))
lines,functions=source['lines'],source['functions'];branches=[b for l in lines for b in l.get('branches',[])]
metrics={'lines':100*sum(l['count']>0 for l in lines)/len(lines),'branches':100*sum(b['count']>0 for b in branches)/len(branches),'functions':100*sum(f['execution_count']>0 for f in functions)/len(functions)}
(OUT/'coverage.json').write_text(json.dumps(metrics,indent=2)+'\n');print(json.dumps(metrics,indent=2))
if any(v<71 for v in metrics.values()):raise SystemExit('Serial wire coverage below71%')
