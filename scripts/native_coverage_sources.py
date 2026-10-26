"""Measure Rust include! fragments with their owning logical production module."""
from functools import cache
import hashlib
import importlib.util
import os
from pathlib import Path
import re


@cache
def rust_parser():
    spec = importlib.util.spec_from_file_location(
        "native_coverage_rust_parser", Path(__file__).with_name("desktop-output-coverage.py"))
    parser = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(parser)
    return parser


@cache
def excluded_lines(source):
    # Instrumentation runs on the host platform. Retain all active production
    # items, including those after interleaved test helpers and imports.
    inactive = r"#\s*\[\s*cfg\s*\(\s*" + (
        r"not\s*\(\s*windows\s*\)" if os.name == "nt" else r"windows") + r"\s*\)\s*\]"
    platform_source = re.sub(inactive, "#[cfg(test)]", source)
    return rust_parser().test_only_lines(platform_source)


@cache
def production_code(source):
    excluded = excluded_lines(source)
    return "\n".join("" if number in excluded else line
                     for number, line in enumerate(rust_parser()._rust_code(source).splitlines(), 1))


def includes(path, production_only=False):
    source = path.read_text(encoding="utf-8")
    matches = list(re.finditer(r'\binclude\s*!\s*\(\s*"([^"\n]+)"\s*\)', source))
    if not matches:
        return
    code = rust_parser()._rust_code(source)
    excluded = excluded_lines(source) if production_only else set()
    for match in matches:
        if not re.match(r"include\s*!", code[match.start():]):
            continue  # Text in a comment or string is not an include directive.
        if source.count("\n", 0, match.start()) + 1 in excluded:
            continue
        yield (path.parent / match[1]).resolve()


def expand_includes(paths, production_only=False):
    """Return each actual source once; fail closed on missing or cyclic includes."""
    found = set()
    active = set()
    result = []

    def visit(path):
        path = Path(path).resolve()
        if path in active:
            raise RuntimeError(f"Cyclic Rust include in coverage sources: {path}")
        if path in found:
            return
        if not path.is_file():
            raise RuntimeError(f"Missing Rust coverage source: {path}")
        found.add(path)
        active.add(path)
        result.append(path)
        if path.suffix == ".rs":
            for child in includes(path, production_only):
                visit(child)
        active.remove(path)

    for path in paths:
        visit(path)
    return result


def production_sources(source):
    return [path for path in expand_includes([source], production_only=True)
            if production_code(path.read_text(encoding="utf-8")).strip()]


def source_stamp(paths, root):
    """Pin recursive production and test fragments, even when only roots are gated."""
    return {str(path.relative_to(root)): {
        "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
        "modifiedNs": path.stat().st_mtime_ns,
    } for path in expand_includes(paths)}


def production_counts(source, annotated):
    excluded = excluded_lines(source)
    counts = {}
    for line in annotated.splitlines():
        match = re.match(r"^\s*(\d+)\|\s*([0-9]+(?:\.[0-9]+)?[kMGT]?)\s*\|", line)
        if match and int(match[1]) not in excluded:
            number = int(match[1])
            if not 1 <= number <= len(source.splitlines()):
                raise RuntimeError("LLVM coverage line is outside the requested source")
            counts[number] = counts.get(number, False) or float(match[2].rstrip("kMGT")) > 0
    return counts


def measure_module(source, annotate, root=None):
    """Aggregate real per-file LLVM output; absent executable fragments fail closed.

    annotate(path) must request that exact filename from llvm-cov show. A facade
    may have zero executable lines, but its executable includes must be present.
    """
    covered = total = 0
    fragments = {}
    annotations = []
    for path in production_sources(source):
        text = path.read_text(encoding="utf-8")
        annotated = annotate(path)
        annotations.append(f"{path}:\n{annotated}")
        counts = production_counts(text, annotated)
        if not counts and re.search(r"\bfn\s+\w+\s*(?:<[^>]*>)?\s*\(", production_code(text)):
            raise RuntimeError(f"No production coverage recorded for executable fragment {path}")
        hits = sum(counts.values())
        covered += hits
        total += len(counts)
        name = str(path.relative_to(root)) if root else str(path)
        fragments[name] = {"covered": hits, "lines": len(counts)}
    if not total:
        raise RuntimeError(f"No production coverage recorded for {source}")
    return {"covered": covered, "lines": total,
            "percent": round(100 * covered / total, 2), "fragments": fragments}, "\n".join(annotations)
