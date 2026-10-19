#!/usr/bin/env python3
"""Measure portable desktop-output modules without constructing a native injector."""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile


def run(arguments, root, env=None):
    result = subprocess.run([str(arg) for arg in arguments], cwd=root, env=env,
                            capture_output=True, text=True, encoding="utf-8")
    if result.returncode:
        print(result.stdout + result.stderr, flush=True)
        result.check_returncode()
    return result.stdout


def _rust_code(source):
    """Mask comments/literals while preserving offsets, newlines and delimiters."""
    masked = list(source)
    position = 0
    while position < len(source):
        end = position
        if source.startswith("//", position):
            end = source.find("\n", position)
            end = len(source) if end < 0 else end
        elif source.startswith("/*", position):
            end, depth = position + 2, 1
            while end < len(source) and depth:
                if source.startswith("/*", end):
                    depth, end = depth + 1, end + 2
                elif source.startswith("*/", end):
                    depth, end = depth - 1, end + 2
                else:
                    end += 1
        elif raw := re.match(r'(?:br|r)(#{0,255})"', source[position:]):
            closing = '"' + raw[1]
            found = source.find(closing, position + raw.end())
            end = len(source) if found < 0 else found + len(closing)
        elif source[position] == '"':
            end = position + 1
            while end < len(source):
                if source[end] == "\\":
                    end += 2
                elif source[end] == '"':
                    end += 1
                    break
                else:
                    end += 1
        elif character := re.match(r"'(?:\\(?:x[0-9a-fA-F]{2}|u\{[0-9a-fA-F_]+\}|.)|[^\\'\n])'", source[position:]):
            end = position + character.end()
        if end > position:
            for index in range(position, min(end, len(source))):
                if source[index] != "\n":
                    masked[index] = " "
            position = end
        else:
            position += 1
    return "".join(masked)


def _test_item_end(code, start):
    """Find one attributed item, never the remainder of its enclosing scope."""
    braced = code[start:start + 1] == "{" or re.match(
        r"(?:pub(?:\s*\([^)]*\))?\s+)?(?:(?:async|unsafe|const|default|extern)\s+)*"
        r"(?:fn|mod|struct|enum|trait|impl|union)\b", code[start:])
    stack = []
    for position in range(start, len(code)):
        token = code[position]
        if token in "([{":
            stack.append(token)
        elif token in ")]}":
            if not stack or stack.pop() != {")": "(", "]": "[", "}": "{"}[token]:
                return None
            if braced and token == "}" and not stack:
                return position + 1
        elif not stack and (token == ";" or (token == "," and not braced)):
            return position + 1
    return None


def test_only_lines(source):
    """Exclude complete cfg(test) items, including fields, blocks and modules.

    An external test module may have intervening #[path] or other attributes.
    No module boundary truncates later production. Mixed production/test lines
    remain in the denominator; unfamiliar syntax is retained conservatively.
    """
    code = _rust_code(source)
    spans = []
    for attribute in re.finditer(r"#\s*\[\s*cfg\s*\(\s*test\s*\)\s*\]", code):
        position = attribute.end()
        while True:
            while position < len(code) and code[position].isspace():
                position += 1
            following = re.match(r"#\s*\[", code[position:])
            if not following:
                break
            position += following.end()
            depth = 1
            while position < len(code) and depth:
                depth += (code[position] == "[") - (code[position] == "]")
                position += 1
        end = _test_item_end(code, position)
        if end is not None:
            spans.append((attribute.start(), end))
    remaining = list(code)
    for start, end in spans:
        for position in range(start, end):
            if remaining[position] != "\n":
                remaining[position] = " "
    excluded, offset = set(), 0
    for number, line in enumerate("".join(remaining).splitlines(keepends=True), 1):
        if not line.strip() and any(start < offset + len(line) and end > offset for start, end in spans):
            excluded.add(number)
        offset += len(line)
    return excluded


def production_counts(source, annotated):
    excluded = test_only_lines(source)
    return {int(match[1]): float(match[2].rstrip("kMGT")) > 0 for line in annotated.splitlines()
            if (match := re.match(r"^\s*(\d+)\|\s*([0-9]+(?:\.[0-9]+)?[kMGT]?)\|", line))
            and int(match[1]) not in excluded}


def main():
    root = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output-dir", type=Path, default=root / ".research/desktop-output-coverage")
    args = parser.parse_args()
    output = args.output_dir.resolve()
    output.mkdir(parents=True, exist_ok=True)
    profiles = Path(tempfile.mkdtemp(prefix="profiles-", dir=output))
    temporary = output / "test-tmp"
    temporary.mkdir(exist_ok=True)
    env = dict(os.environ, CARGO_TARGET_DIR=str(output / "target"),
               RUSTFLAGS="-C instrument-coverage", TEMP=str(temporary), TMP=str(temporary),
               LLVM_PROFILE_FILE=str(profiles / "%m-%p.profraw"))
    artifacts = run(["cargo", "test", "--locked", "--lib", "--no-run", "--message-format=json",
                     "--manifest-path", root / "crates/desktop-output/Cargo.toml"], root, env)
    binaries = [entry["executable"] for line in artifacts.splitlines()
                if (entry := json.loads(line)).get("reason") == "compiler-artifact"
                and entry.get("executable") and entry.get("profile", {}).get("test")]
    if len(binaries) != 1:
        raise RuntimeError("Expected one desktop-output library test executable.")
    (output / "tests.log").write_text(run([binaries[0]], root, env), encoding="utf-8")
    sysroot = Path(run(["rustc", "--print", "sysroot"], root).strip())
    host = re.search(r"^host: (.+)$", run(["rustc", "-vV"], root), re.M).group(1)
    suffix = ".exe" if os.name == "nt" else ""
    llvm = sysroot / "lib/rustlib" / host / "bin"
    profile = output / "coverage.profdata"
    run([llvm / f"llvm-profdata{suffix}", "merge", "-sparse", *profiles.glob("*.profraw"), "-o", profile], root)
    summary = {}
    # Native process entry, Win32 injection and receiver probes have a separate
    # interactive acceptance boundary. Coverage here cannot qualify that path.
    for module in ("keys", "protocol", "reducer", "worker", "coordinator"):
        source = root / f"crates/desktop-output/src/{module}.rs"
        annotated = run([llvm / f"llvm-cov{suffix}", "show", binaries[0], f"-instr-profile={profile}",
                         "-show-line-counts-or-regions", source], root)
        (output / f"{module}-lines.txt").write_text(annotated, encoding="utf-8")
        counts = production_counts(source.read_text(encoding="utf-8"), annotated)
        if not counts:
            raise RuntimeError(f"No instrumented production lines for {module}.")
        covered, total = sum(counts.values()), len(counts)
        summary[module] = dict(covered=covered, lines=total, percent=round(100 * covered / total, 2))
        print(f"{module}: {covered}/{total} ({summary[module]['percent']}%)", flush=True)
    (output / "summary.json").write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
    if any(module["percent"] < 71 for module in summary.values()):
        raise SystemExit("Each portable desktop-output module must meet 71% production line coverage.")


if __name__ == "__main__":
    main()
