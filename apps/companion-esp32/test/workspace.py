"""Fault-inject the actual ESP32 workspace/pool paths for both memory profiles."""
from pathlib import Path
import os
import subprocess
import json
import re
import gzip

APP=Path(__file__).resolve().parents[1]
OUT=APP.parents[1]/'.research/esp32-workspace-native'
OUT.mkdir(parents=True,exist_ok=True)
for rich in (0,1):
    flags=['-std=c11','-O0','-g','--coverage','-Wall','-Wextra','-Werror','-fsanitize=address,undefined','-fno-omit-frame-pointer','-DESP_PLATFORM',f'-DOSG_TEST_RICH={rich}']
    flags+=['-I'+str(APP/p) for p in ['test/workspace_include','network','render','core','third_party/cjson']]
    sources=['network/workspace.c','network/reply_buffer.c','network/render_profile.c','render/lvgl_pool.c','third_party/cjson/cJSON.c','test/workspace_test.c']
    binary=OUT/('workspace-'+str(rich))
    for counter in OUT.glob(binary.name+'-*.gcda'):
        counter.unlink()
    subprocess.run([os.environ.get('CC','gcc'),*flags,*[str(APP/p) for p in sources],'-o',str(binary)],check=True)
    report=OUT/('profile-'+str(rich)+'.json')
    subprocess.run([str(binary),str(report)],check=True,env={**os.environ,'UBSAN_OPTIONS':'halt_on_error=1:print_stacktrace=1'})
    manifest=json.loads((APP.parents[1]/'packages/companion-designer/assets/widget-profiles.json').read_text())
    expected={key:value for key,value in manifest.items() if key!='profiles'}
    expected.update(manifest['profiles']['rich' if rich else 'compact'])
    version=re.search(r'#define OSG_FIRMWARE_VERSION "([^"]+)"',(APP/'network/firmware.h').read_text()).group(1)
    expected.update({'firmwareVersion':version,'boardProfile':'headless','screen':{'width':800,'height':480}})
    assert json.loads(report.read_text())==expected
    subprocess.run(['gcov','-j',str(binary)+'-workspace.gcno'],check=True,cwd=OUT,stdout=subprocess.DEVNULL)
    coverage=json.loads(gzip.decompress((OUT/(binary.name+'-workspace.gcov.json.gz')).read_bytes()))
    lines=coverage['files'][0]['lines']
    percent=100*sum(line['count']>0 for line in lines)/len(lines)
    assert percent>=71,percent
    print(f'Workspace profile {rich}: {percent:.2f}% line coverage')
