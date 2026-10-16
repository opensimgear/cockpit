"""Host fault injection for the Sunton shared-SPI adapter."""
from pathlib import Path
import os, subprocess
root=Path(__file__).resolve().parents[3]
app=root/"apps/companion-esp32"
out=root/".research/esp32-sunton-native"
out.mkdir(parents=True,exist_ok=True)
subprocess.run([os.environ.get("CC","gcc"),"-std=c11","-Wall","-Wextra","-Werror","-pedantic","-g","-fsanitize=address,undefined",
    "-I"+str(app/"test/sunton_include"),"-I"+str(app/"boards"),"-I"+str(app/"core"),str(app/"core/diagnostics.c"),str(app/"test/sunton_test.c"),"-o",str(out/"board-test")],check=True)
result=subprocess.run([str(out/"board-test")],check=True,capture_output=True,text=True)
(out/"board-test.log").write_text(result.stdout+result.stderr)
print(result.stdout,end="")
