# Windows journal inspection race — 3 November 2026

Fixes #200, discovered during #188/R9 qualification. This resolves a storage
failure rather than expanding the reactive feature scope.

## Evidence and change

The recurring `Cannot inspect persistence path.` message discarded the failing
operation, leaf and Windows error. A temporary, opt-in diagnostic build captured
`check_path`, `parent=false`, `PermissionDenied`, raw OS error **5/ACCESS_DENIED**
on **cockpit.sqlite3-journal**. The main database and ancestors were not the
failing operation. Simple create/save/delete cycles passed; a planned 100-cycle
internal-profile lifecycle probe failed late at Apply's retained-cleanup guard.
Later recovery inspection contained no outputs, provisioning or diagnostics and
`required=false`. That guard failure was a transient failed inspection, not an
actual retained output record.

The pinned SQLite runtime uses DELETE journal mode and handles some transient
Windows inspection/deletion errors in its VFS. Rust's Windows metadata fallback
can return its original access error when the journal disappears concurrently.
Our pre-open metadata check ran before SQLite could handle that condition.

The fix explicitly identifies the database's journal leaf. On Windows error 5
only, it permits three fresh metadata inspections after requested waits of
1/2/4 ms. It never infers absence from access denial and never retries a SQL,
profile or output command. Every wait is followed by immediate main/ancestor
validation before another leaf inspection. Successful metadata must still pass
the existing reparse/type checks; genuine NotFound retains its existing meaning.
Persistent denial and all other errors fail. WAL/SHM leaf policy and generic
main/ancestor validation remain strict. Connections and live evidence share this
sidecar policy. The temporary diagnostic tracing is not shipped.

Ordinary successful inspection does not wait. The delays are requested backoff,
not a claim of precise Windows scheduling or an input latency measurement.
Persistence remains outside input/telemetry/output callbacks.

## Verification

- **64 persistence tests pass**, zero failures, one existing explicit ignore.
  Six new deterministic Windows cases cover disappearance, successful reinspection,
  bounded persistent denial, unrelated errors, immediate failure after main-path
  replacement, wrong types and a real junction after an injected transient error.
- **All 14 persistence production-module coverage gates pass**; the new journal
  module is 100%, path validation 95.83%, connection 94.03%, live evidence 97.22%.
- **2,050 native tests pass**, zero failures, 17 existing explicit ignores. All
  11 coverage-source verifier tests pass.
- The rebuilt actual Cockpit completes **200 create/save/apply/stop/delete cycles**
  with zero command retries, no physical/external outputs, no cleanup errors,
  stopped final ownership and clear retained-recovery status.
- All **10 surrounding native checks pass**: controller catalogue sharing,
  games subscriptions, artwork/setup/native file changes, scoped baselines,
  authored pulse and script expiry, preview sharing and physical-reader
  replacement/Stop. This includes both previously failing deadline fixtures.
- Authored DCS ownship/BIOS and all four authored Assetto processing/expiry/recovery
  fixtures pass. These are software fixtures, not live simulator qualification.
- Production native build passes. The tested candidate is isolated from existing
  dirty workspace work; staged native sources and manifests are compared against
  it before committing, and unrelated files are checked for byte preservation.

Executable SHA-256:
`86A630BDEE06C896A56D9E2FFB98B790B7FD5185C1A86B5994BFC948219836DB`.
Local evidence is under `.research/reactive/persistence-race-*`; the strict
regression is `tests/ui/persistence-concurrency-native.mjs` with
`OSG_PERSISTENCE_CONCURRENCY_NATIVE=1`.

## Remaining work

#188/R9 remain open. Workspace automation/source ownership and the remaining
device, companion, settings/activity, adapter/routing, UI and MCP migrations still
need implementation and qualification. #199 remains first in progress for
user-operated acceptance; physical timing is unqualified and Assetto sampling is
unapproved. CI remains deferred until all features are implemented. The previous
unsupported overall percentage estimate remains withdrawn.
