"""Fault-inject the actual ESP32 workspace/pool paths for both memory profiles."""
from pathlib import Path
import os
import subprocess

APP=Path(__file__).resolve().parents[1]
OUT=APP.parents[1]/'.research/esp32-workspace-native'
OUT.mkdir(parents=True,exist_ok=True)
for rich in (0,1):
    flags=['-std=c11','-O1','-g','-Wall','-Wextra','-Werror','-fsanitize=address,undefined','-fno-omit-frame-pointer','-DESP_PLATFORM',f'-DOSG_TEST_RICH={rich}']
    flags+=['-I'+str(APP/p) for p in ['test/workspace_include','network','render','core','third_party/cjson']]
    sources=['network/workspace.c','network/reply_buffer.c','render/lvgl_pool.c','test/workspace_test.c']
    binary=OUT/('workspace-'+str(rich))
    subprocess.run([os.environ.get('CC','gcc'),*flags,*[str(APP/p) for p in sources],'-o',str(binary)],check=True)
    subprocess.run([str(binary)],check=True,env={**os.environ,'UBSAN_OPTIONS':'halt_on_error=1:print_stacktrace=1'})
