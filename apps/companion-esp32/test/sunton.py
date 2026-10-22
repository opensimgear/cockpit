"""Host fault injection for the Sunton shared-SPI adapter."""
from pathlib import Path
import os, subprocess, gzip, json
root=Path(__file__).resolve().parents[3]
app=root/"apps/companion-esp32"
out=root/".research/esp32-sunton-native"
out.mkdir(parents=True,exist_ok=True)
for counter in out.glob('board-test-*.gcda'):
    counter.unlink()
subprocess.run([os.environ.get("CC","gcc"),"-std=c11","-Wall","-Wextra","-Werror","-pedantic","-g","--coverage","-fsanitize=address,undefined",
    "-I"+str(app/"test/sunton_include"),"-I"+str(app/"boards"),"-I"+str(app/"core"),str(app/"test/sunton_test.c"),"-o",str(out/"board-test")],check=True)
result=subprocess.run([str(out/"board-test")],check=True,capture_output=True,text=True)
(out/"board-test.log").write_text(result.stdout+result.stderr)
print(result.stdout,end="")
subprocess.run(['gcov','-j',str(out/'board-test-sunton_test.gcno')],check=True,cwd=out,stdout=subprocess.DEVNULL)
coverage=json.loads(gzip.decompress((out/'board-test-sunton_test.gcov.json.gz').read_bytes()))
lines=next(source['lines'] for source in coverage['files'] if source['file'].endswith('/board.c'))
percent=100*sum(line['count']>0 for line in lines)/len(lines)
assert percent>=71,percent
print(f'Sunton driver: {percent:.2f}% line coverage')
