from pathlib import Path
import json,shlex,subprocess
root=Path(__file__).resolve().parents[1]
wanted={'app_main.c','engine.c','scene.c','expressions.c','content_store.c','content_store_esp.c','raster.c','client.c','board.c','esp_lcd_bridge.c'}
for board in ('sunton','waveshare'):
    out=root/'.research/target-stack'/board;out.mkdir(parents=True,exist_ok=True)
    commands=json.loads((root/f'apps/companion-esp32/build-engine-{board}/compile_commands.json').read_text())
    for command in commands:
        path=Path(command['file'])
        if path.name not in wanted or '/apps/companion-esp32/' not in str(path):continue
        args=command.get('arguments') or shlex.split(command['command']);args=list(args)
        args[args.index('-o')+1]=str(out/(path.parent.name+'-'+path.name+'.o'));args.append('-fstack-usage')
        subprocess.run(args,cwd=command['directory'],check=True,stdout=subprocess.DEVNULL)
    report=[]
    for p in sorted(out.glob('*.su')):
        for line in p.read_text().splitlines():
            fields=line.split('\t');n=int(fields[1]);
            if n>=128:report.append((n,line))
    (out/'frames.txt').write_text('\n'.join(line for n,line in sorted(report,reverse=True))+'\n')
    print(board);print((out/'frames.txt').read_text())