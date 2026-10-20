#!/usr/bin/env python3
"""Inspect, prepare and validate independent component releases. No publish side effects."""

from __future__ import annotations

import argparse
import datetime as dt
import json
import re
import subprocess
import sys
from pathlib import Path


COMPONENTS = ("cockpit", "esp32-companion", "mobile-companion")
VERSION_PATTERN = r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)"
TAG_RE = re.compile(r"(" + "|".join(COMPONENTS) + r")-v(" + VERSION_PATTERN + r")\Z")
VERSION_RE = re.compile(VERSION_PATTERN + r"\Z")
ENTRY_RE = re.compile(r"^## ([^\n]+)$", re.MULTILINE)
CONFIG_PATH = "release/components.json"


class ReleaseError(ValueError):
    pass


def run(root: Path, *args: str, input_text: str | None = None) -> str:
    result = subprocess.run(args, cwd=root, input=input_text, text=True,
                            encoding="utf-8", capture_output=True, check=False)
    if result.returncode:
        raise ReleaseError(f"{' '.join(args)} failed: {result.stderr.strip() or result.stdout.strip()}")
    return result.stdout.strip()


def version_tuple(version: str) -> tuple[int, int, int]:
    if not VERSION_RE.fullmatch(version):
        raise ReleaseError(f"Invalid stable SemVer: {version!r}; expected X.Y.Z without leading zeros")
    return tuple(int(part) for part in version.split("."))


def bump_version(version: str, change: str) -> str:
    major, minor, patch = version_tuple(version)
    if change == "breaking":
        return f"{major + 1}.0.0" if major else f"0.{minor + 1}.0"
    if change == "feature":
        return f"{major}.{minor + 1}.0"
    if change == "fix":
        return f"{major}.{minor}.{patch + 1}"
    raise ReleaseError(f"Unknown change classification: {change}")


def parse_tag(tag: str) -> tuple[str, str]:
    match = TAG_RE.fullmatch(tag)
    if not match:
        raise ReleaseError("Invalid release tag; expected cockpit-vX.Y.Z, esp32-companion-vX.Y.Z, or mobile-companion-vX.Y.Z")
    return match.group(1), match.group(2)


def local_path(root: Path, relative: str) -> Path:
    path = (root / relative).resolve()
    if not path.is_relative_to(root.resolve()):
        raise ReleaseError(f"Path escapes repository: {relative}")
    return path


def load_config(root: Path) -> dict:
    config = json.loads(local_path(root, CONFIG_PATH).read_text(encoding="utf-8"))
    if config.get("schemaVersion") != 1 or set(config.get("components", {})) != set(COMPONENTS):
        raise ReleaseError("Unsupported release configuration")
    for component, data in config["components"].items():
        version_tuple(data["version"])
        if data["tagPrefix"] != f"{component}-v":
            raise ReleaseError(f"Unexpected tag prefix for {component}")
        for path in [data["changelog"], *data["sourcePaths"], *data["sharedPaths"],
                     *(binding["path"] for binding in data["versionFiles"])]:
            local_path(root, path)
    return config


def binding_match(text: str, binding: dict) -> re.Match:
    kind = binding["kind"]
    if kind == "json":
        parsed = json.loads(text)
        if not isinstance(parsed.get("version"), str):
            raise ReleaseError(f"Missing JSON version in {binding['path']}")
        pattern = r'("version"\s*:\s*")([^"\n]+)(")'
        matches = list(re.finditer(pattern, text))
        if len(matches) != 1 or matches[0].group(2) != parsed["version"]:
            raise ReleaseError(f"Ambiguous JSON version in {binding['path']}")
        return matches[0]
    if kind in ("cargo-package", "cargo-lock"):
        block_pattern = (r"(?ms)^\[package\]\s*\n(.*?)(?=^\[|\Z)" if kind == "cargo-package"
                         else r"(?ms)^\[\[package\]\]\s*\n(.*?)(?=^\[\[|\Z)")
        blocks = [match for match in re.finditer(block_pattern, text)
                  if re.search(r'^name\s*=\s*"' + re.escape(binding["package"]) + r'"\s*$', match.group(), re.MULTILINE)]
        if len(blocks) != 1:
            raise ReleaseError(f"Expected exactly one {binding['package']} package in {binding['path']}")
        matches = list(re.finditer(r'(?m)^(version\s*=\s*")([^"\n]+)(")', text))
        matches = [match for match in matches if blocks[0].start() <= match.start() < blocks[0].end()]
    elif kind == "c-macro":
        matches = list(re.finditer(r'(?m)^(#define\s+' + re.escape(binding["macro"]) + r'\s+")([^"\n]+)(")', text))
    else:
        raise ReleaseError(f"Unknown version binding: {kind}")
    if len(matches) != 1:
        raise ReleaseError(f"Expected one version in {binding['path']}")
    return matches[0]


def with_version(text: str, binding: dict, version: str) -> str:
    match = binding_match(text, binding)
    return text[:match.start(2)] + version + text[match.end(2):]


def entry_text(changelog: str, version: str) -> str:
    headings = list(ENTRY_RE.finditer(changelog))
    matches = [index for index, heading in enumerate(headings)
               if heading.group(1).split(" - ", 1)[0] == version]
    if len(matches) != 1:
        raise ReleaseError(f"Expected exactly one changelog entry for {version}")
    index = matches[0]
    heading = headings[index]
    end = headings[index + 1].start() if index + 1 < len(headings) else len(changelog)
    entry = changelog[heading.start():end].strip()
    if not re.fullmatch(re.escape(version) + r" - \d{4}-\d{2}-\d{2}", heading.group(1)):
        raise ReleaseError("Release heading must be '## X.Y.Z - YYYY-MM-DD'")
    try:
        dt.date.fromisoformat(heading.group(1).split(" - ")[1])
    except ValueError as error:
        raise ReleaseError("Invalid release date") from error
    sections = list(re.finditer(r"(?m)^### (.+)$", entry))
    names = [match.group(1) for match in sections]
    if names not in (["Added", "Modified", "Fixed"], ["Added", "Modified", "Fixed", "Removed"]):
        raise ReleaseError("Changelog sections must be Added, Modified, Fixed, and optionally Removed, in that order")
    if entry.split("\n", 1)[1].split("###", 1)[0].strip():
        raise ReleaseError("Put release notes in the changelog sections")
    meaningful = False
    for index, section in enumerate(sections):
        end = sections[index + 1].start() if index + 1 < len(sections) else len(entry)
        body = entry[section.end():end].strip()
        lines = body.splitlines()
        if not lines or not any(line.startswith("- ") for line in lines):
            raise ReleaseError(f"Section {section.group(1)} needs bullets or '- None.'")
        if any(line and not line.startswith(("- ", "  ")) for line in lines):
            raise ReleaseError(f"Invalid content in section {section.group(1)}")
        if any(line.startswith("- ") and not line[2:].strip() for line in lines):
            raise ReleaseError(f"Empty bullet in section {section.group(1)}")
        has_items = any(line.startswith("- ") and line != "- None." for line in lines)
        if section.group(1) == "Removed" and not has_items:
            raise ReleaseError("Omit Removed when nothing was removed")
        meaningful |= has_items
    if not meaningful:
        raise ReleaseError("Release notes need at least one meaningful change")
    return entry + "\n"


def validate(root: Path, tag: str, notes_output: Path | None = None) -> dict[str, str]:
    component, version = parse_tag(tag)
    data = load_config(root)["components"][component]
    if data["version"] != version:
        raise ReleaseError(f"Tag version {version} does not match {component} metadata {data['version']}")
    for binding in data["versionFiles"]:
        current = binding_match(local_path(root, binding["path"]).read_text(encoding="utf-8"), binding).group(2)
        if current != version:
            raise ReleaseError(f"{binding['path']} has version {current}, expected {version}")
    notes = entry_text(local_path(root, data["changelog"]).read_text(encoding="utf-8"), version)
    if notes_output is not None:
        notes_output.parent.mkdir(parents=True, exist_ok=True)
        notes_output.write_text(notes, encoding="utf-8")
    return {"component": component, "version": version, "tag": tag,
            "notes_file": str(notes_output) if notes_output is not None else data["changelog"]}


def latest_release(root: Path, component: str) -> dict | None:
    # Query published releases, not the repository-wide /latest endpoint or local tags.
    pages = run(root, "gh", "api", "--paginate", "--slurp", "repos/{owner}/{repo}/releases?per_page=100")
    releases = [release for page in json.loads(pages) for release in page]
    matching = []
    for release in releases:
        if release.get("draft") or not release.get("published_at"):
            continue
        try:
            tag_component, _ = parse_tag(release["tag_name"])
        except ReleaseError:
            continue
        if tag_component == component:
            matching.append(release)
    return max(matching, key=lambda release: release["published_at"], default=None)


def inspect_component(root: Path, component: str, explicit_baseline: str | None = None) -> dict:
    data = load_config(root)["components"][component]
    release = latest_release(root, component)
    if release is not None:
        if explicit_baseline:
            raise ReleaseError("An explicit baseline is only supported before the first component release")
        baseline = release["tag_name"]
        baseline_kind = "published-release"
        _, released_version = parse_tag(baseline)
    elif explicit_baseline:
        baseline = explicit_baseline
        baseline_kind = "explicit-first-release-baseline"
        released_version = None
    else:
        baseline = run(root, "git", "hash-object", "-t", "tree", "--stdin", input_text="")
        baseline_kind = "repository-root"
        released_version = None
    if baseline_kind != "repository-root":
        # Resolve to a commit before accepting arbitrary tag/ref input.
        commit = run(root, "git", "rev-parse", "--verify", baseline + "^{commit}")
        run(root, "git", "merge-base", "--is-ancestor", commit, "HEAD")
    paths = list(dict.fromkeys(data["sourcePaths"] + data["sharedPaths"]))
    head = run(root, "git", "rev-parse", "HEAD")
    diff_args = ["git", "diff", "--name-status", baseline, head, "--", *paths]
    log_range = head if baseline_kind == "repository-root" else f"{baseline}..{head}"
    return {
        "component": component, "configured_version": data["version"],
        "released_version": released_version, "baseline": baseline,
        "baseline_kind": baseline_kind, "head": head, "paths": paths,
        "release_url": release.get("html_url") if release else None,
        "changed_files": run(root, *diff_args).splitlines(),
        "commits": run(root, "git", "log", "--format=%h %s", log_range, "--", *paths).splitlines(),
        "diff_stat": run(root, "git", "diff", "--stat", baseline, head, "--", *paths),
    }


def require_clean_main(root: Path) -> None:
    if run(root, "git", "branch", "--show-current") != "main":
        raise ReleaseError("Prepare releases on main")
    if run(root, "git", "status", "--porcelain", "--untracked-files=all"):
        raise ReleaseError("Release preparation needs a clean committed main; preserve and finish unrelated work first")


def prepare(root: Path, plans: dict, dry_run: bool = False) -> dict:
    require_clean_main(root)
    config = load_config(root)
    changes: dict[Path, str] = {}
    results = []
    skipped = []
    for component, plan in plans.items():
        if component not in COMPONENTS:
            raise ReleaseError(f"Unknown component: {component}")
        if not isinstance(plan, dict):
            raise ReleaseError(f"{component} plan must be an object")
        data = config["components"][component]
        evidence = inspect_component(root, component, plan.get("baseline"))
        if "skip" in plan:
            if plan["skip"] is not True or not isinstance(plan.get("reason"), str) or not plan["reason"].strip():
                raise ReleaseError(f"{component} skip plan requires skip: true and a nonempty reason")
            if set(plan) - {"skip", "reason", "baseline"}:
                raise ReleaseError(f"{component} skip plan cannot also specify release metadata")
            if evidence["changed_files"]:
                raise ReleaseError(f"Cannot skip {component}: its scoped diff is not empty")
            skipped.append({"component": component, "reason": plan["reason"].strip(),
                            "baseline": evidence["baseline"], "baseline_kind": evidence["baseline_kind"]})
            continue
        version = plan["version"]
        version_tuple(version)
        released = evidence["released_version"]
        if released and version_tuple(version) <= version_tuple(released):
            raise ReleaseError(f"{component} version must be newer than published {released}")
        if version_tuple(version) < version_tuple(data["version"]):
            raise ReleaseError(f"{component} version cannot go backwards from configured {data['version']}")
        if not evidence["changed_files"]:
            raise ReleaseError(f"No changes in {component}'s release scope since its baseline")
        tag = data["tagPrefix"] + version
        # Never replace a pre-existing local tag, even if no GitHub release was published.
        existing = subprocess.run(["git", "show-ref", "--verify", "--quiet", f"refs/tags/{tag}"], cwd=root)
        if existing.returncode == 0:
            raise ReleaseError(f"Tag {tag} already exists; inspect its release before retrying")
        if existing.returncode not in (0, 1):
            raise ReleaseError("Cannot check existing release tag")
        notes_path = Path(plan["notesFile"])
        if not notes_path.is_absolute():
            notes_path = root / notes_path
        supplied_notes = notes_path.read_text(encoding="utf-8").strip()
        notes = entry_text(supplied_notes, version)
        if supplied_notes != notes.strip():
            raise ReleaseError("Notes input must contain exactly the proposed release entry")
        changelog_path = local_path(root, data["changelog"])
        changelog = changelog_path.read_text(encoding="utf-8")
        if any(heading.group(1).split(" - ", 1)[0] == version for heading in ENTRY_RE.finditer(changelog)):
            raise ReleaseError(f"Changelog already contains {component} {version}")
        title, _, previous = changelog.partition("\n")
        changes[changelog_path] = title + "\n\n" + notes + ("\n" + previous.strip() + "\n" if previous.strip() else "")
        for binding in data["versionFiles"]:
            path = local_path(root, binding["path"])
            changes[path] = with_version(path.read_text(encoding="utf-8"), binding, version)
        data["version"] = version
        results.append({"component": component, "version": version, "tag": tag,
                        "baseline": evidence["baseline"], "baseline_kind": evidence["baseline_kind"]})
    if not results:
        raise ReleaseError("No components need a release; nothing was prepared")
    changes[local_path(root, CONFIG_PATH)] = json.dumps(config, indent=2) + "\n"
    if not dry_run:
        originals = {path: path.read_text(encoding="utf-8") for path in changes}
        try:
            for path, content in changes.items():
                path.write_text(content, encoding="utf-8")
            for result in results:
                validate(root, result["tag"])
        except Exception:
            for path, original in originals.items():
                path.write_text(original, encoding="utf-8")
            raise
    return {"dry_run": dry_run, "releases": results, "skipped": skipped,
            "tags": [result["tag"] for result in results],
            "files": [path.relative_to(root).as_posix() for path in changes]}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    commands = parser.add_subparsers(dest="command", required=True)
    inspect_parser = commands.add_parser("inspect", help="Read published baseline and committed change evidence")
    inspect_parser.add_argument("component", choices=(*COMPONENTS, "all"))
    inspect_parser.add_argument("--baseline", help="Explicit legacy baseline before the first matching component release")
    prepare_parser = commands.add_parser("prepare", help="Update only versions and changelog; never commit, tag or push")
    prepare_parser.add_argument("component", choices=(*COMPONENTS, "all"))
    prepare_parser.add_argument("--version")
    prepare_parser.add_argument("--notes-file")
    prepare_parser.add_argument("--baseline")
    prepare_parser.add_argument("--plan-file", type=Path, help="For all: map each component to version/notesFile or skip:true/reason, with optional baseline")
    prepare_parser.add_argument("--dry-run", action="store_true")
    validate_parser = commands.add_parser("validate", help="Validate tagged version and export only its changelog entry")
    validate_parser.add_argument("tag")
    validate_parser.add_argument("--notes-output", type=Path)
    validate_parser.add_argument("--github-output", type=Path)
    bump_parser = commands.add_parser("bump", help="Calculate SemVer from a change classification")
    bump_parser.add_argument("version")
    bump_parser.add_argument("change", choices=("breaking", "feature", "fix"))
    args = parser.parse_args()
    root = args.root.resolve()
    try:
        if args.command == "validate":
            output = args.notes_output
            if output is not None and not output.is_absolute():
                output = root / output
            result = validate(root, args.tag, output)
            if args.github_output:
                with args.github_output.open("a", encoding="utf-8") as stream:
                    for key, value in result.items():
                        if "\n" in value or "\r" in value:
                            raise ReleaseError("Invalid multiline GitHub output")
                        stream.write(f"{key}={value}\n")
        elif args.command == "inspect":
            if args.component == "all" and args.baseline:
                raise ReleaseError("Inspect each component separately when supplying a legacy baseline")
            selected = COMPONENTS if args.component == "all" else [args.component]
            result = {component: inspect_component(root, component, args.baseline) for component in selected}
        elif args.command == "prepare":
            if args.component == "all":
                if not args.plan_file or args.version or args.notes_file or args.baseline:
                    raise ReleaseError("prepare all requires only --plan-file and optional --dry-run")
                plans = json.loads(args.plan_file.read_text(encoding="utf-8"))
                if not isinstance(plans, dict) or set(plans) != set(COMPONENTS):
                    raise ReleaseError("An all plan must include exactly the three independent components")
            else:
                if not args.version or not args.notes_file or args.plan_file:
                    raise ReleaseError("prepare <component> requires --version and --notes-file")
                plans = {args.component: {"version": args.version, "notesFile": args.notes_file, "baseline": args.baseline}}
            result = prepare(root, plans, args.dry_run)
        else:
            result = {"version": bump_version(args.version, args.change)}
        print(json.dumps(result, indent=2))
        return 0
    except (ReleaseError, OSError, KeyError, TypeError, json.JSONDecodeError) as error:
        print(f"Release error: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
