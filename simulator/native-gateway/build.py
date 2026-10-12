#!/usr/bin/env python3
"""Build the production-C native gateway/module emulator into workspace .research/."""
import argparse
from pathlib import Path
import shutil
import subprocess

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--workspace',type=Path,default=Path(__file__).resolve().parents[3])
    parser.add_argument('--common-source',type=Path)
    parser.add_argument('--sanitize',action='store_true')
    args=parser.parse_args(); workspace=args.workspace.resolve()
    common=(args.common_source or workspace/'.research/generic-pinned-arm-example/firmware_common-src').resolve()
    gateway=workspace/'device-gateway/firmware'; module=workspace/'device-template/firmware'
    cyphal=workspace/'protocol/cyphal'; output=workspace/'.research/native-gateway/native-gateway'
    output.parent.mkdir(parents=True,exist_ok=True)
    sources=[Path(__file__).with_name('native_gateway.c'),
        *[gateway/'src'/name for name in ('gateway.c','gateway_registry.c','gateway_journal.c','gateway_cockpit.c','gateway_config.c','gateway_layout.c')],
        *[module/'src'/name for name in ('can_client.c','can_delivery.c','runtime_config.c','config_can.c','boot_session.c')],
        *[common/'src/inputs'/name for name in ('profile.c','inputs.c','button.c')],common/'src/config/line.c',
        cyphal/'src/cyphal.c',cyphal/'src/gateway_protocol.c',cyphal/'vendor/libcanard/canard.c']
    includes=[gateway/'include',module/'include',module/'profiles',common/'include',cyphal/'include',cyphal/'generated',cyphal/'vendor/libcanard']
    for source in sources:
        if not source.is_file(): parser.error(f'Required source missing: {source}')
    flags=['-std=c11','-O1','-g','-Wall','-Wextra','-Werror','-UNDEBUG']
    if args.sanitize:flags+=['-fsanitize=address,undefined','-fno-omit-frame-pointer']
    subprocess.run([shutil.which('cc'),*flags,*[arg for path in includes for arg in ('-I',str(path))],
                    *map(str,sources),'-o',str(output)],check=True)
    print(output)

if __name__=='__main__':main()
