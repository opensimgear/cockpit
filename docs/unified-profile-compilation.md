# Unified profile compilation

The schema-2 profile graph is the authoring source for a bounded typed program.
Compilation consumes an immutable document and a native inventory capture. It
does not open providers, acquire an output, hide a device, execute a script or
activate a profile. P4 supplies runtime admission and submission; the editor
follows in P6/P7.

## Contracts and execution

`crates/mapping/src/signal.rs` defines the shared scalar, state, event, action and
invocation contracts used by inventory, graph validation and the typed engine.
Numbers retain range and units; text, enums, RGB and ordered action parameters
retain their constraints. Unknown, stale and unavailable are distinct samples.
Action packing and setter changes are explicit nodes; a retained state cannot
authorize a one-shot action. Raw controller axes use the captured native value,
so normalization requires an explicit compatible node.

`program.rs` and `program_validation.rs` validate concrete sources, processing
nodes, sinks and virtual layouts. The typed phase reuses the deterministic clock,
existing button/timing rules and runtime lifecycle. It records virtual reports
and emits typed resource effects for the future native coordinator. Starting a
validated candidate replaces the previous phase; an invalid candidate leaves it
intact. Loss, gaps and new generations withdraw dependencies before timers run.
Script callbacks replace their complete output and timer contributions, and
expired or stale tickets cannot restore them.

All conversions and processing are named graph nodes: range/unit conversion,
rounding, deadzone, inversion, curves, comparison/hysteresis, formatting, gates,
priority, event edges, fallback, existing timing logic and declared scripts. There
are no edge transformations. Logical controller layouts have explicit buttons,
axis ranges/fallbacks and hats; their identities do not encode host backend slots.

## Compiler and capture

`crates/profiles/src/compiler.rs` compiles one document and `CompileSnapshot` into
one report. The plan includes normalized program instructions, typed adapters,
topological order, node/port/edge traces, required/optional branch readiness,
virtual-controller and visibility summaries, and resource budgets. Structural
errors yield diagnostics without an executable plan. Missing native capabilities
can instead leave a valid plan blocked or dormant.

The compiler rejects duplicate physical input routes, ambiguous writers, virtual
outputs captured as inputs and known conflicting virtual/direct simulator panel
bindings. External integration command and telemetry endpoints do not introduce
an internal combinational cycle. The portable program hash includes execution
policies and layouts while excluding names, editor coordinates, live route tokens
and backend slots. A separate capture fingerprint identifies native context.

`profile_resources::Runtime::snapshot_for_compile` reads only cached descriptors
under its inventory lock. It captures exact selected port variants and current
read/write/invoke tokens, preserving canonical resource identity. It performs no
provider collection, device I/O or persistence. Native game and output/hiding
backend capabilities remain unavailable until their implementing milestones;
browser-supplied tokens or qualification flags are rejected.

## Read-only preview API

- `profile_compile { document }` captures a saved profile's local assignments at
  the draft's current base revision and compiles the unsaved document. Bounded,
  shape-valid drafts with semantic errors return inspectable diagnostics.
- `profile_compilation_inspect { compilationId, section, offset, limit }` reads
  diagnostics, nodes, order, sources, sinks, branches, controllers or visibility
  from exactly that immutable preview. The maximum page size is 64 whole items.

The native cache holds four previews, each limited to 8 MiB. The initial response
contains counts and at most 32 diagnostics. Replies stay below 1.9 MiB by returning
short whole-item pages; clients advance by the returned count. Expired IDs require
explicit recompilation. Inspecting a preview never recaptures providers, and its
ID is not an activation authority. Apply must capture and validate again.

`ProfileCompilerClient` validates and freezes response data and rejects mixed
preview identities. A shared native/frontend summary fixture verifies the actual
wire representation, including prefixed SHA-256 values. Preview commands share
the ordered profile queue; inventory and physical discovery remain independent.

## Fresh storage

Graph and library schema 2 use `profiles-v2`; the inventory metadata namespace
remains separate. Older affected configurations are expendable, so no migration
or compatibility reader is introduced. Strict Save/import/export validation stays
separate from compile-only draft parsing. Local resource and exact-port
assignments are revisioned machine-local data and are excluded from portable
exports and copies.
