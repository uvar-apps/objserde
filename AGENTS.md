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

* Repo: `github.com/uvar-apps/objserde` (public, `origin`), branch `main`.
  On crates.io: 0.2.0 (tag `v0.2.0`) and 0.3.0 (published 2026-11-15, from
  `fd5e5e1`). `0.4.0` is committed locally and **not yet published** — each
  publish is a separate, explicit per-version decision of the user. Crates.io
  categories may still need adding via the web UI.
* Toolchain: edition 2024, `rust-version = 1.85` (local 1.100).
* Version `0.4.0`. The 0.3 → 0.4 step is breaking in the same way 0.2 → 0.3 was:
  `Obj` gained `smooth_groups` and `Face` gained `smooth_group`, so downstream
  struct literals without `..Default::default()` fail to compile (as in 0.1 → 0.2,
  when `Obj` gained `materials` and `material_libs` and `Face` gained `material`,
  and 0.2 → 0.3 with `header`/`objects`/`groups`). Smoothing values are stored
  verbatim and never interpreted; Mode A writes them where they change, in the
  order `o`, `g`, `s`, `usemtl`, and a model with no smoothing writes none — so
  output for smoothing-free files is unchanged byte for byte.
* Tests: 117 without features (119 with `serde`) — 26 parser contract, 23 MTL
  parser/writer, 20 materials/OBJ integration, 16 ported loader/sample, 11
  binding, 6 round-trip properties, 11 writer, 3 doctests, plus 2 gating the
  `serde` derives. Green with and without `--features serde`, in debug and
  release, **and on an unpacked `cargo package` tarball**, where the Stanford
  models are absent and their tests skip.
* Performance: same proxy, idle M-series — read 15–16 ms, parse 399–413 ms, bind
  21–26 ms, **parse + bind 420–435 ms** against the 1.2 s budget; `Obj::load` end
  to end 485–503 ms. Material support measured A/B against `ac35e00`: no
  difference beyond noise.
  Objects, groups and header (0.3) measured A/B against `fe21aeb` on a 92 MB,
  1.28 M-face sesmesh surface (parse + Mode A write, best of 5, alternating,
  load average ≈ 7): 0.453–0.456 s → 0.464–0.470 s, about 2–3 % — the two extra
  `Option<u32>` per face.
* **Timing discipline (learned the hard way):** measure on an idle machine, and
  A/B against a baseline worktree (`git worktree add /tmp/base <sha>`) before
  crediting or blaming a change. Figures recorded in this repo's first draft were
  taken during concurrent dependency builds and were ~2x slow — a fabricated
  regression and a fabricated fix, both from the same mistake.

## Documented decisions — materials (0.2)

* **`.mtl` parsing is total.** `parse_mtl` returns `Vec<Material>`, never a
  `Result`; a tag the model cannot hold goes to `Material::unknown`, verbatim, and
  the writer reproduces it. Rationale: broken paint must not make a file
  unopenable. `parse_mtl_file` is the strict path only in the I/O sense.
* **`mtllib` resolves automatically in `parse_file`**, against the OBJ's
  directory; `parse`/`parse_bytes` leave it `LibraryStatus::Unresolved` for an
  explicit `Obj::resolve_material_libs(base)`. Absent → `Missing`, present but
  unreadable → `Unreadable`. Neither is an error, both are recorded: never let
  "no materials" and "materials not found" collapse into the same value.
* **Resolution is by name.** First definition of a name wins; a `usemtl` name no
  loaded library defines keeps a `defined: false` entry so its faces retain the
  reference; `usemtl` before `mtllib` still resolves.
* **A repeated tag inside one material keeps the last value** (renderers do the
  same); a repeated `newmtl` name keeps the **first** block. Different rules for
  different scopes — both documented and tested.
* **`Tr` is stored, not converted to `d`**; unstated tags stay `None` rather than
  zero. Both are readings, not opinions.
* **The writer emits `usemtl` per change, not per face**, quotes `mtllib` paths
  containing whitespace, and skips `defined: false` entries (the reference lives
  in the OBJ line, and re-reading re-synthesizes it).
* **Map tag case and the `bump`/`map_bump` spelling are canonicalized**, the way
  the OBJ writer canonicalizes `1.0` to `1`.
* **No hash map anywhere.** Material names are matched linearly, on `usemtl` lines
  only — dozens of comparisons per file, and it keeps iteration order out of the
  read/write path entirely rather than relying on an argument that a particular
  map is not iterated.
* **Texture paths are returned to the caller and nothing else.** `Map.path` is the
  string from the tag, verbatim. objserde does not resolve it against any
  directory, does not `stat` it, and does not decode it — no image dependency, no
  syscall storm, no wrong base directory turning "missing texture" into a lie.
  Deciding what a texture path is *relative to*, and whether the file exists, is
  the renderer's job and belongs to whichever resource root the app owns. Adding
  texture resolution, existence status, or decoding here is out of scope by
  decision, not by omission (pinned by
  `tests/materials.rs::texture_paths_are_stored_not_resolved`). This does not
  extend to `mtllib`: those paths *are* resolved, because a model genuinely cannot
  know its colours without them.
* **Layer modules (`bind`, `error`, `model`, `mtl`, `parse`, `write`) are public
  paths**; the root keeps re-exporting the common types, so `objserde::Material`
  and `objserde::mtl::Material` are the same item. No new items, just navigable
  docs for the three-layer story.

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
* **`o`, `g` and the header comments are modelled since 0.3**
  (`Obj::objects`/`Face::object`, `Obj::groups`/`Face::group`, `Obj::header`).
  `o`, `g` and `s` are state like `usemtl`: tables in first-appearance order, the
  value kept as written (several names on one line stay one string, a bare keyword
  is `""`, and `s off` / `s 0` / `s 12` are three distinct uninterpreted strings),
  written where they change in the order `o`, `g`, `s`, `usemtl`. Names are looked
  up through a `BTreeMap` during the parse (ordered, never iterated, no hashing),
  because a file can carry thousands of groups. The header is the comments before
  the first record, text after `#` verbatim, written back first.
* **Later comments / `vp` / unknown lines are skipped and not round-tripped.** A
  documented limit, not an oversight. (`usemtl`/`mtllib` left this list in 0.2,
  `o`/`g`/header comments in 0.3, `s` in 0.4.)
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
  --all-targets -- -D warnings` (with and without `--features serde`),
  `cargo test --release` (both configs), and `cargo doc --no-deps` clean — public
  modules make broken intra-doc links easy to introduce. Then `cargo package` and
  test the unpacked tarball: that is the only check that the published artifact
  still works without the excluded assets. Note `cargo test` does **not** build
  feature-gated test files unless the feature is on, so a `--features serde` run
  is not optional.
- Performance budget: a 1M-vertex / 2M-face OBJ parses + binds in ≤ 1.2 s on
  M-series, release (measured 420–435 ms on a 149 MB proxy, idle machine).
  Regenerate with `python3 assets/perf/gen_million_sphere.py`; the check is
  `cargo test --release --test perf -- --ignored --nocapture`, which skips when
  the proxy is absent.
