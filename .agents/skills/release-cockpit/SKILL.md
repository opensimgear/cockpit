---
name: release-cockpit
description: Prepare or publish independently versioned Cockpit desktop, ESP32 Companion firmware, and Mobile Companion releases from this repository, with diff-based changelogs and tag-triggered CI. Use for an actual release or a release dry run; creating or changing the release tooling itself does not authorize a release.
---

# Release Cockpit

Run from the Cockpit repository root. Supported release selections are `cockpit`, `esp32-companion`, `mobile-companion`, and `all`. If the request says only “release Cockpit”, select `cockpit`; `all` prepares three independently versioned releases and three tags, never a suite version.

`cockpit` contains Windows, Linux, and macOS desktop builds. `esp32-companion` contains Waveshare and Sunton firmware. `mobile-companion` contains Android and an iOS simulator build; label simulator downloads as simulator-only. Check the release workflow for current packaging details and preserve those limitations in release notes when relevant.

## Inspect and choose versions

1. Read applicable `AGENTS.md` instructions and `release/components.json`. Each component owns its version, tag prefix, changelog, source/shared dependency scope, and version bindings. Initial `0.1.0` values are manifest/first-release seeds, not evidence of a published release. Existing date-based ESP32 firmware is migrated to SemVer only when preparing its first release.
2. Use committed `main` with no pending tracked or untracked work. Inspect `git status --short` before changing anything. Never stash, discard, stage, commit, or absorb unrelated work to make a release possible. If work is pending, finish the authorized work separately or report the blocker. Use the running dev Cockpit for appropriate smoke verification under the repository instructions.
3. Fetch `origin` main and tags. Confirm local `main` is up to date with `origin/main`, with no unexplained local commits. Run `python scripts/release.py inspect <component>` (or `all`). It finds the most recently **published** GitHub release matching each component's strict tag prefix, including shared dependencies, and checks that its tag is an ancestor of `HEAD`. Authentication/API failures are errors, never evidence that no release exists.
4. Read the actual diff and relevant source, not just commit subjects: use the returned baseline/head/paths with `git diff <baseline> <head> -- <paths...>` and `git log`. For `all`, repeat independently for all three baselines. Explain shared changes according to their effect on each component; do not copy desktop changes into firmware/mobile notes without evidence.
5. If no matching component release exists, inspect the repository's legacy releases/tags. Use `inspect <component> --baseline <legacy-tag>` only when an appropriate published legacy release is established. Otherwise the helper compares against the empty repository tree and includes the initial implementation. State that this is the first component release and identify the chosen baseline; never invent previous release history.
6. Choose SemVer from the public behavior in the diff: breaking compatibility increments major after 1.0; before 1.0 it increments minor; compatible additions increment minor; fixes and compatible maintenance increment patch. Reset lower fields on a major/minor bump. Use `python scripts/release.py bump <previous-version> breaking|feature|fix` if useful. Start at `0.1.0` when no release or higher existing version establishes a floor. Respect an explicit user version if valid and explain its effect. Skip empty changes for a single-component request. For `all`, mark a component with an empty scoped diff as skipped in the plan below. If files changed but none warrant a release, explain that distinction and resolve the requested scope; do not use the empty-diff skip marker or invent a changelog entry.

## Prepare and review

Write proposed notes under ignored `.research/` and use this exact shape, with the release date in the user's timezone:

```markdown
## 0.2.0 - 2026-10-20

### Added
- Describe a user-visible addition, or None.

### Modified
- Describe changed behavior, or None.

### Fixed
- Describe a corrected problem, or None.
```

Use `- None.` for a section with no entries. Add `### Removed` last only when something was removed. Group related work, describe observable outcomes, and keep technical maintenance proportionate to its effect. At least one meaningful entry is required. The release pipeline uses the exact committed version entry as GitHub release notes and Discord content.

For one component:

```text
python scripts/release.py prepare cockpit --version 0.2.0 --notes-file .research/cockpit-notes.md --dry-run
python scripts/release.py prepare cockpit --version 0.2.0 --notes-file .research/cockpit-notes.md
```

Pass `--baseline <legacy-tag>` on prepare too if inspect established that first-release baseline. For `all`, create `.research/release-plan.json` mapping exactly the three component names to objects with independently selected `version`, `notesFile`, and optional first-release `baseline`. A component whose scoped diff is empty instead uses `{"skip": true, "reason": "No changes since its published release"}` (plus optional first-release `baseline`). Skip plans have no version or notes file. The helper rechecks that its scoped diff is empty; shared dependency changes count as changes. At least one component must be prepared. If every component is unchanged, report that there is nothing to release without running prepare.

Run `prepare all --plan-file .research/release-plan.json --dry-run`, then the same command without `--dry-run`. The helper uses one clean-main guard and preflights every component before changing any file. It returns `releases`, `skipped`, `tags`, and the exact affected `files`; skipped components retain their version and changelog. It synchronizes only prepared component versions and changelogs, including each component's own Cargo lockfile package record. This single preparation avoids sequential commands failing after the first component makes the working tree dirty.

If the user requested **prepare** or **dry run**, stop before commit/tag/push. A dry run must not mutate tracked files; show proposed versions, notes, baselines, and affected files. A prepare request may leave the reviewed version/changelog edits locally. Creating this skill or editing release infrastructure never means “release now”.

## Publish an authorized release

An actual “release” request authorizes the following concrete release operations without a second permission question. Complete preparation and relevant checks first. Review `git diff --check`, the full diff, and `python -m unittest discover -s scripts -p test_release.py -v`. Validate each intended tag with `python scripts/release.py validate <component>-vX.Y.Z --notes-output .research/<component>-release-notes.md`.

Stage **only** paths returned by prepare. Inspect the staged diff, commit the version/changelog changes on `main`, and verify the working tree is clean. For `all`, one atomic release preparation commit contains the independently versioned components returned in `releases`. Create annotated tags only for those returned in `tags`, for example `git tag -a cockpit-v0.2.0 -m "Cockpit 0.2.0"`; skipped components get no tag. Never move or overwrite an existing tag.

Push the branch and all prepared tags together: `git push --atomic origin main refs/tags/cockpit-v0.2.0` (include every returned tag ref for `all`). A release tag triggers CI; ordinary branch pushes run checks without creating a release. If push fails, stop and report the local commit/tags and exact error; do not force-push, silently change the version, or delete/retarget tags. Follow each triggered workflow to completion, inspect failures, and report the published release URLs and artifact availability, together with any skipped components. A rerun for the same tag must use the same committed notes and version. Discord uses the website's release webhook convention and may skip when its secret is absent; inspect the workflow summary.
