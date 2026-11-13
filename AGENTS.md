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
- **Semantic parity with meshview is checked bitwise, not by eye.** After any
  change to `bind.rs` (or to float parsing), rebuild the throwaway parity crate
  that depends on both `objserde` and `meshview` by path and compare
  `meshview::load_obj` with `objserde::Obj::load` on all three samples using
  `f64::to_bits` per component. As of this commit all positions, indices and
  unit normals are bit-identical. objserde itself must never depend on meshview.

## Commands

```sh
cargo build --release
cargo test --release                       # also: --features serde
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo package --list
cargo publish --dry-run                    # do NOT publish
```

## Status

* Branch `main`, not pushed and not published — remote operations are the
  maintainer's. The crates.io name `objserde` was verified free at authoring
  time (API and sparse index both 404).
* Toolchain: edition 2024, `rust-version = 1.85` (local 1.100).
* Tests: 65 without features (67 with `serde`) — 22 parser contract, 16 ported loader/sample, 11 binding, 6
  round-trip properties, 9 writer, 1 doctest, 2 gate the `serde` derives. Green with and without
  `--features serde`, in debug and release, **and on an unpacked
  `cargo package` tarball**, where the Stanford models are absent and their
  tests skip.
* Performance: 149 MB / 1,001,112-vertex / 1,999,396-face proxy — parse
  834–922 ms, bind 31–59 ms, **874–962 ms** together against the 1.2 s budget
  (five consecutive release runs, M-series).

## Documented decisions (deliberate — do not "fix" them)

Each of these sharpens or differs from the meshview reference on purpose, and
each is tested. Changing one needs a semver bump, a README table edit, and a
line here.

* **Relative (`-3`) indices are refused in all three corner fields.** meshview
  rejected them only for `v` and silently left a negative `vn` unbound, so a file
  using relative normals loaded and got geometric normals instead of the ones it
  asked for, without a word. `bind` stays total; the parser just never hands it an
  index it cannot represent.
* **`0` in a `vt`/`vn` field means "unset" → stored as absent.** `0` in the
  position field is an error.
* **Only `v` indices are range-validated.** An out-of-range `vn` is kept as
  written and left unbound by `bind` (rule 2 of the contract); an out-of-range
  `vt` is kept for whoever consumes texture coordinates.
* **An index past the `u32` address space** is reported as `IndexOutOfRange`
  where it appears, with the vertex count as read so far — the one place a
  reported count is not the final table size.
* **Fields past the third on `vn` and `vt` are dropped**; on `v` they are kept in
  `Vertex::extra`.
* **An `f ` line with no corner tokens produces no face**, and the writer emits
  nothing for a hand-built 0-corner `Face`, so writer output always re-parses.
* **Comments / `usemtl` / `g` / `o` / `s` / unknown lines are skipped and not
  round-tripped** in v1. A documented limit, not an oversight.
* **Parse errors name the line, not the file** (only `Io` carries a path), which
  is what keeps errors cheap. Callers add the path they know.

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
