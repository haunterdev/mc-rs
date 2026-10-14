"""Phase 0: build ../mc-1.12.2-src from the vanilla 1.12.2 jars already in the Gradle cache.

Writes, next to this repo (never inside it):
  mc-1.12.2-src/src/          vanilla sources, SRG names renamed to MCP stable_39
  mc-1.12.2-src/server-mcp.jar the vanilla server jar remapped to MCP names (runs the harness)

Inputs, all from GRADLE_USER_HOME (RetroFuturaGradle and ForgeGradle caches):
  retro_futura_gradle/mc-vanilla/1.12.2/server.jar  checked against Mojang's sha1
  retro_futura_gradle/fernflower-cache/*.jar        RFG's fernflower output of vanilla, SRG names
  minecraft/de/oceanlabs/mcp/mcp_stable/39/         fields.csv, methods.csv, params.csv, notch-mcp.srg
  modules-2/.../org.ow2.asm                         ASM for the remapper
"""
import csv, glob, hashlib, json, os, re, shutil, subprocess, sys, zipfile
from pathlib import Path

from jdk import find_jdk8

REPO = Path(__file__).resolve().parent.parent
OUT = REPO.parent / "mc-1.12.2-src"
GRADLE = Path(os.environ.get("GRADLE_USER_HOME", Path.home() / ".gradle")) / "caches"
JDK8 = find_jdk8()
MCP = GRADLE / "minecraft/de/oceanlabs/mcp/mcp_stable/39"
VANILLA = GRADLE / "retro_futura_gradle/mc-vanilla"


def verify_server_jar():
    manifest = json.loads((VANILLA / "manifest_1.12.2.json").read_text())
    want = manifest["downloads"]["server"]["sha1"]
    jar = VANILLA / "1.12.2/server.jar"
    got = hashlib.sha1(jar.read_bytes()).hexdigest()
    if got != want:
        sys.exit(f"server.jar sha1 {got} does not match Mojang's {want}")
    print(f"server.jar sha1 {got} matches the 1.12.2 manifest")
    return jar


def find_decompiled():
    for jar in glob.glob(str(GRADLE / "retro_futura_gradle/fernflower-cache/*.jar")):
        with zipfile.ZipFile(jar) as z:
            if "net/minecraft/world/gen/ChunkGeneratorOverworld.java" in z.namelist():
                return Path(jar)
    sys.exit("no RFG fernflower output containing ChunkGeneratorOverworld.java")


def load_names():
    names = {}
    for f in ("fields.csv", "methods.csv"):
        with open(MCP / f, newline="", encoding="utf-8") as fh:
            for row in csv.DictReader(fh):
                names[row["searge"]] = row["name"]
    with open(MCP / "params.csv", newline="", encoding="utf-8") as fh:
        for row in csv.DictReader(fh):
            names[row["param"]] = row["name"]
    return names


def rename_sources(src_jar, names):
    token = re.compile(r"\b(?:field_\d+_[a-zA-Z]+_?|func_\d+_[a-zA-Z]+_?|p_[\w]+_\d+_)\b")
    dest = OUT / "src"
    if dest.exists():
        shutil.rmtree(dest)
    count = 0
    with zipfile.ZipFile(src_jar) as z:
        for name in z.namelist():
            if not name.endswith(".java"):
                continue
            text = z.read(name).decode("utf-8")
            text = token.sub(lambda m: names.get(m.group(0), m.group(0)), text)
            path = dest / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(text, encoding="utf-8", newline="\n")
            count += 1
    print(f"wrote {count} MCP-named sources to {dest}")


def remap_jar(server_jar):
    asm = sorted(glob.glob(str(GRADLE / "modules-2/files-2.1/org.ow2.asm/asm/9.8/*/asm-9.8.jar")))[0]
    commons = sorted(glob.glob(str(GRADLE / "modules-2/files-2.1/org.ow2.asm/asm-commons/9.8/*/asm-commons-9.8.jar")))[0]
    build = OUT / "tools"
    build.mkdir(parents=True, exist_ok=True)
    cp = os.pathsep.join([asm, commons])
    subprocess.run([str(JDK8 / "bin/javac"), "-d", str(build), "-cp", cp, str(REPO / "tools/remap/Remap.java")], check=True)
    srg = MCP / "1.12.2/srgs/notch-mcp.srg"
    out = OUT / "server-mcp.jar"
    subprocess.run([str(JDK8 / "bin/java"), "-cp", os.pathsep.join([cp, str(build)]), "Remap", str(server_jar), str(srg), str(out)], check=True)
    print(f"wrote {out}")


if __name__ == "__main__":
    OUT.mkdir(exist_ok=True)
    server = verify_server_jar()
    rename_sources(find_decompiled(), load_names())
    remap_jar(server)
