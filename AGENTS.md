# AGENTS.md — objserde

Instructions and invariants for coding agents working on this repository. Read
this before changing anything: the rules here exist because violating them made
a mesh comparison lie once already, in the code this crate was distilled from.

## What this is

`objserde` is a single home for the Wavefront OBJ read/write code the
maintainer kept re-writing across projects. Three layers, deliberately
separate:

1. **Raw loss-preserving model** (`model.rs` + `parse.rs`) — `Obj` keeps `v`
   lines (including trailing floats), the `vn` table as written, `vt` arity,
   and polygons unfan-ed.
2. **Binding policy** (`bind.rs`) — the fixed **uvar** normal-binding contract:
   bind by `vn` *index*, first reference wins, geometric fallback for unbound
   vertices, renormalize by `max(‖n‖, 1e-12)`. Total function, no error path.
3. **Writers** (`write.rs`) — Mode A structural round-trip (`Obj::to_string`),
   Mode B canonical mesh form compatible with `molses` (`write_mesh`).

Upstream references, **read-only forever**: `/Users/ugo/projects/meshview`
(the reader semantics) and `/Users/ugo/projects/SES/crates/molses` (the writer
line grammar). Do not modify either; do not depend on either.

## Standing rules

- **std-only core.** `serde` is optional (feature flag), `proptest` is
  dev-only. No GPU/rendering deps, no `nom`, no `thiserror`, no `ahash`.
  Errors are hand-rolled `Display + std::error::Error`.
- **The parser skips, never dies on, record types it doesn't model.** An
  unknown line prefix is not an error. Death is reserved for a record it *does*
  model whose fields are unreadable.
- **`bind` semantics are the uvar contract:** index binding, first-wins,
  geometric fallback, `max(‖n‖, 1e-12)` renormalization, total function. Any
  change needs a **semver bump** plus a **README semantics-table update**.
- **Writers stay byte-deterministic.** Same model in ⇒ same bytes out, always.
  Never introduce `HashMap`/`HashSet` ordering, wall-clock timestamps, or
  locale-dependent formatting near a writer — comparisons and caches downstream
  rely on this.
- **SES and meshview checkouts are read-only forever.**

## Commands

```sh
cargo build --release
cargo test --release                       # also: --features serde
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo package --list
cargo publish --dry-run                    # do NOT publish
```

## Assets and licensing (hard rule)

Code is `MIT OR Apache-2.0`; both LICENSE files stay in the repo root.
`assets/samples/` carries its own provenance `README.md`, kept verbatim.

The **Stanford bunny and dragon are research/educational-licensed scan data
(Turk/Levoy)**: they live in the git repository for tests but must **never**
enter a published crate — they are listed in `Cargo.toml` `exclude`. Any test
needing them must skip gracefully when the file is absent (see
`tests/common/mod.rs::have`) so a registry checkout still builds and tests
green. `1crn_ses_0.5A.obj` (molses, MIT OR Apache-2.0, from unrestricted PDB
entry 1CRN) ships with the crate.

## Working rules

- Commit often, clear messages, `main` only. Do not push, tag, or publish
  unless the user explicitly asks in that turn — approval in one turn does not
  carry to the next.
- Gate before reporting done: `cargo fmt --check`, `cargo clippy --workspace
  --all-targets -- -D warnings`, `cargo test --release`, with and without
  `--features serde`.
- Performance budget: a 1M-vertex / 2M-face OBJ parses + binds in ≤ 1.2 s on
  M-series, release (measured 863 ms on a 149 MB proxy). Regenerate with
  `python3 assets/perf/gen_million_sphere.py`; the check is
  `cargo test --release --test perf -- --ignored --nocapture`, which skips when
  the proxy is absent.
