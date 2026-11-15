# objserde

Parse and generate Wavefront OBJ in Rust: a **loss-preserving parser**, **MTL
materials with their shading and texture references**, a **fixed-policy
normal-binding layer**, and **byte-deterministic writers**, in one `std`-only
crate with zero required dependencies.

```toml
[dependencies]
objserde = "0.3"
```

It exists because the same OBJ reader and writer kept being rewritten per
project, and because the two halves — reading a file without quietly changing
the mesh, and writing one that another tool reads identically — have to agree on
one set of semantics. They are the same code now.

```rust
// Read, bind normals, shade.
let mesh = objserde::Obj::load("1crn_ses_0.5A.obj")?;
assert_eq!(mesh.vertex_count(), 37_930);
assert_eq!(mesh.triangle_count(), 75_852);

// Keep everything, change nothing: raw model in, raw model out.
let obj = objserde::parse_file("model.obj")?;
let same = objserde::parse(&obj.to_string())?;
assert_eq!(same, obj);

// Materials and textures come with the file: `mtllib` is read from the same
// directory, and each face knows the material it was drawn under.
for face in &obj.faces {
    if let Some(mat) = obj.material_of(face) {
        let _colour: Option<[f64; 3]> = mat.diffuse;
        let _texture: Option<&objserde::Map> = mat.diffuse_map();
    }
}

// Generate a mesh in the canonical form, normals and all.
let text = objserde::write_mesh(&mesh, &objserde::WriteOptions {
    header: vec!["# my-tool 1.0".into(), "# units: angstrom".into()],
    ..Default::default()
});
```

## Three layers, deliberately separate

Everything a file says lives in layer 1. Interpreting a normal lives in layer 2.
Producing bytes lives in layer 3. They do not blend into each other, which is
what makes the middle one reviewable as a policy rather than discovered as an
accident inside a loop.

| | Type | Holds | Never does |
|---|---|---|---|
| **1 · raw model** | [`Obj`](https://docs.rs/objserde/latest/objserde/struct.Obj.html) | `v` lines with trailing floats, the `vn` table exactly as written (not unit), `vt` arity, whole polygons, each face's material, object and group, the `mtllib` references and the materials they resolved to, the header comments | resolve a normal against a surface point |
| **2 · binding** | `BoundMesh` | positions, one unit normal per position, triangle indices | fail, or look at a material |
| **3 · writers** | `obj.to_string()`, `write_mesh`, `write_material_lib` | deterministic bytes | add a timestamp, read a locale |

Layer 2 is a policy with a name — the **uvar binding contract** — not a data
structure. If you need different semantics, parse with layer 1 and resolve
normals yourself; nothing in this crate stops you, and nothing in it will do it
differently than documented.

## Semantics

### Which lines are read

| Line | Modelled as | Notes |
|---|---|---|
| `v x y z …` | `Obj::vertices` | Position plus **any** trailing floats: `xyzrgb`-family vertex colours survive a round trip |
| `vn x y z …` | `Obj::normals` | Stored **as written**, not normalized; fields past the third are ignored |
| `vt u [v [w]]` | `Obj::tex_coords` | 1, 2 or 3 components; the count is kept so it round-trips |
| `f c₁ c₂ …` | `Obj::faces` | Polygons, not pre-triangulated; 1- and 2-corner faces are kept |
| `mtllib <files>` | `Obj::material_libs` | Each path, with whether it could be read; several files may share one line, and `"quoted names"` may contain spaces |
| `usemtl <name>` | `Obj::materials`, `Face::material` | State: it applies to this face and every face after it, until the next `usemtl` |
| `o <name>` | `Obj::objects`, `Face::object` | State, like `usemtl`; the table holds each name once, in order of first appearance |
| `g <names>` | `Obj::groups`, `Face::group` | State, like `usemtl`; the names as written (`g hull left` → `"hull left"`, a bare `g` → `""`), each distinct line once |
| `# …` before the first record | `Obj::header` | The text after `#`, verbatim; blank lines among them are dropped. Later comments are skipped |

Prefixes are matched **exactly**: `v` must be followed by a space. So `vertex …`,
`vnotavertex`, `v\t1 2 3` and ` v 1 2 3` (leading space) are not vertex lines —
they are skipped, like every other line: later comments, `s`, `vp`, blanks,
unknown prefixes. Skipping is never an error, because an OBJ reader that dies on a
record it doesn't model can't read most files in the wild.

### Corner forms

`f` accepts the four standard forms and records which one each corner used:
`i`, `i/vt`, `i//vn`, `i/vt/vn` — all stored 0-based. In `a/b/c`, `b` is a
texture coordinate and is never read as a normal; a normal exists only when the
third field is present and non-empty. Fields past the third are ignored, as the
reference reader ignores them. An index of `0` in the `vt`/`vn` slot is OBJ's
"unset" marker and is stored as absent.

### Fan triangulation

`Obj::triangulate()` (and `bind()`) split polygons into corners
`(0, k, k+1)` for `k in 1..n-1`, in file order. A face with fewer than three
corners produces no triangles **but stays in the model**, so writing the file
back reproduces it.

### Materials and textures

A `.mtl` file is a flat list of `newmtl` blocks, and the model mirrors it without
interpreting anything:

| Tag | Field | Notes |
|---|---|---|
| `Ka` `Kd` `Ks` `Ke` | `ambient` `diffuse` `specular` `emissive` | `Option<[f64; 3]>` — an unstated tag is **not** a zero |
| `Ns` | `shininess` | specular exponent |
| `d` / `Tr` | `dissolve` / `transmission` | kept separately; `Tr` is not converted into `d` |
| `illum` | `illumination` | the model index, stored not enforced |
| `map_Ka` `map_Kd` `map_Ks` `map_Ke` `map_Ns` `map_d` `map_bump`/`bump` `disp` `decal` | `maps: Vec<Map>` | each with its `path` and `-s -o -bm -imfchan -blendu -blendv -clamp -texres` options, in file order |
| anything else | `unknown: Vec<UnknownTag>` | tag and arguments verbatim, written back unchanged |

A texture is a path. `Map.path` is exactly what the tag said; objserde returns it
and stops — no resolving against a directory, no existence check, no decoding.
What a texture path is relative to belongs to the renderer and its resource root.
(`mtllib` paths are the exception and *are* resolved, because without the
definition the model does not know its own colours.)

Three properties worth knowing, all of them tested:

**Parsing a material file cannot fail.** `parse_mtl` returns materials, never a
`Result`. A malformed `Kd` is broken paint, not broken geometry, and letting it
fail `Obj::load` would turn a missing texture into a file that will not open.
Anything the model cannot hold is preserved in `unknown` instead of dropped, so
the leniency costs no data: `Kd 0.44 wood.spe` (a spectral colour reference) and a
vendor's `sharpini 90` both survive to the writer.

**Library resolution is automatic and honest about failure.** `parse_file` reads
each `mtllib` path relative to the OBJ's directory. `parse` and `parse_bytes`
cannot, so their references come back `Unresolved` until you call
`obj.resolve_material_libs(dir)`. A library that is absent is `Missing`, one that
exists but cannot be read is `Unreadable` — recorded, never fatal, because "this
model has no materials" and "its materials were not found" are different facts a
caller may act on.

**Names, not positions, identify a material.** `usemtl` matches by string, so a
reference resolves whether it appears before or after its `mtllib` line; the first
definition of a name wins; and a name no loaded library defines keeps a
`defined: false` entry so the faces that asked for it do not silently lose their
assignment. Material names may contain spaces, on both sides of the reference.

### Normal binding — the five rules

1. A `vn` binds to the **position that references it, by index**; the first
   corner to reference a position wins.
2. A `vn` index outside the normal table leaves the corner **unbound** — it is
   not an error.
3. Unbound positions get **area-weighted geometric normals**: each triangle adds
   its unnormalized `(b − a) × (c − a)` to all three corners, accumulated in
   triangle order so the float sum is reproducible.
4. Everything is then divided by `max(‖n‖, 1e-12)`.
5. Binding is **total**: it cannot fail.

Rule 4 has a consequence worth stating: a zero-length vector stays **exactly**
zero. The Stanford dragon has genuine zero-area vertices, an unnormalized
0/0 would poison them with NaN, and a vertex with no orientation should read as
having no orientation rather than being invented.

### Errors

| Error | Fires when |
|---|---|
| `NoVertices` | The file parsed and had no `v` line at all |
| `BadFloat { line, got }` | A numeric field could not be read; `got` is the offending token, or `<missing field>` if the line ended early |
| `NegativeIndex { line, value }` | A corner used a relative (`-3`) index, or `0` for its position |
| `IndexOutOfRange { line, value, count }` | A position index past the vertex table, or an index past the addressable `u32` space |
| `Io { path, source }` | The file could not be read |

Every parse error names the line it happened on, and errors never carry the
buffer — a failed 149 MB parse does not allocate a 149 MB message.

Face position indices are validated **at end of parse**, against the finished
table. That is deliberate: it means an out-of-range face on line 40 reports line
40, and no early failure hides the rest. Relative indices are refused rather
than wrapped — the Python original silently wrapped them as Python does, which
produced a mesh that looked plausible and was wrong.

Materials add **no** error variants. Reading an `.mtl` is total (see above), and a
library that will not load is a status on `Obj::material_libs`, not an `Err` — the
only filesystem failure a caller sees is `Io` for the OBJ itself.

## The binding war story, condensed

Flat-shaded exports write a `vn` per face *corner*, so their `vn` indices do not
line up with their `v` indices at all. Bind by arrival order instead of by index
and every vertex ends up with *some* normal from the neighbourhood: the silhouette
is unchanged, the shading is subtly wrong, and a side-by-side comparison of two
such meshes reports a difference that is not in the geometry.

That is how one comparison once lied. The symptoms were a checkerboard of
background speckles, the meshes got blamed for a while, and the bug was in the
loader's assumption about which table an index addressed. Two rules came out of
it and both are load-bearing here: **normals bind by the corner's `vn` index,
never by arrival order**, and **a corner that cannot be read is reported, never
silently substituted** — which is why this crate refuses relative indices in any
field where the reference reader quietly dropped one, and why an unreadable corner
is an error with a line number rather than a shrug.

Both are pinned by tests that fail loudly if reintroduced:
`vn_binds_by_index_not_arrival_order`, `vt_corners_fall_back_to_geometric_normals`,
`first_referencing_corner_wins`, `out_of_range_vn_leaves_the_corner_unbound`,
`zero_vn_stays_exactly_zero`.

## Writers

### Mode A — structural round trip (`obj.to_string()`)

Writes every record in canonical layout — the header comments, then all `v`, then
`vt`, then `vn`, then `f`, with `o`, `g` and `usemtl` lines where they change —
keeping each record's shape: trailing floats on `v`, `vt` arity, and the corner
form each corner used.

Guaranteed: `parse(&obj.to_string())` yields **bit-identical values**, including
`-0.0`. Not guaranteed, and not claimed: byte equality with the file it was read
from (skipped records are gone, record order is normalized, `1.0` is written `1`),
or byte equality with some other producer's output.

### Mode B — canonical mesh form (`write_mesh`)

The form `molses` emits, reproduced line for line:

```
v {:.6} {:.6} {:.6}
vn {:.6} {:.6} {:.6}
f a//a b//b c//c          # 1-based
```

`WriteOptions { precision, header }` chooses `Precision::Fixed(6)` (the default,
and the `molses` format) or `Precision::Shortest`, and supplies `#`-style header
lines written verbatim. Corner form is normalized to `i//i` here — this mode
generates fresh meshes; use Mode A to preserve a file's shape.

### Material libraries (`write_material_lib`)

`write_material_lib(&obj.materials, &MtlWriteOptions { header, .. })` writes the
`.mtl` text: one `newmtl` block per defined material, tags in canonical order,
then that material's maps in the order they were read, then any `unknown` tags
verbatim. Numbers use the shortest form that parses back to the same value, so
`parse_mtl(&write_material_lib(m, &o))` equals `m` bit for bit. Tag *case* is
canonicalized (`MAP_KD` → `map_Kd`) and `bump` becomes `map_bump`.

Materials that only stand in for an unloaded reference (`defined: false`) are not
written — a definition file should not assert properties it does not have. The
reference itself is safe: it lives in the OBJ's `usemtl` line, and re-reading
recreates the entry.

Mode A writes the OBJ half of the same story: `mtllib` lines at the top (quoted
when the path contains spaces) and a `usemtl` line wherever the material changes —
one per change, not one per face, because in OBJ a material is state.

### The determinism promise

Same model in, **identical bytes out**, always, on every run and every machine.
Nothing in either writer iterates a hash map, reads a clock, or formats through a
locale. Comparisons, cache keys and regression checks downstream are built on
this; `tests/write.rs` and the `to_string_is_deterministic` property enforce it.

## Performance

`cargo build --release`, Apple silicon, single thread, idle machine. A UV-sphere
proxy of 1,001,112 vertices / 1,999,396 triangles / 149 MB, in `f i//i` form
(`assets/perf/gen_million_sphere.py`), over five consecutive runs:

| Stage | Time |
|---|---|
| read | 15–16 ms |
| parse | 399–413 ms |
| bind | 21–26 ms |
| **parse + bind** | **420–435 ms** (budget 1.2 s) |
| `Obj::load` end to end | 485–503 ms |

Most of the parse time is `f64` field conversion, and the model pays one
`Vec<Corner>` allocation per face because keeping polygons whole — rather than
pre-triangulating into flat index arrays — is exactly what layer 1 exists to do.

Material support cost nothing measurable: the same proxy was timed on the
pre-material commit and the current one, interleaved, and the difference sat
inside run-to-run noise (~2 ms). An earlier revision of this table claimed
~830–920 ms; those runs were taken while large dependency builds were competing
for the machine. Two lessons, both now in `AGENTS.md`: measure on an idle
machine, and A/B a *baseline commit* before crediting or blaming a change.

## Parity with the reference loader

`objserde::bind` and meshview's `load_obj` produce **bit-identical** output on all
three samples: same positions, same triangle indices, and same unit normals down
to the last bit (0 of 113,790 / 107,841 / 375,198 normal components differ). This
is the real test of a rewrite that is supposed to preserve hard-won semantics, so
it is worth re-running whenever the binding layer changes.

The check cannot live in this crate's test suite — objserde must not depend on
meshview — so it runs from a throwaway crate outside the repo:

```toml
[dependencies]                      # /tmp/paritycheck/Cargo.toml
objserde = { path = "/Users/ugo/projects/objserde" }
meshview = { path = "/Users/ugo/projects/meshview" }  # read-only reference
```

then compare `meshview::load_obj(p)` against `objserde::Obj::load(p)` with
`f64::to_bits` per component.

## Limits

Stated because knowing them is the difference between a tool and a surprise.

* **Skipped records do not round-trip.** Comments after the first record, `s`,
  `vp` and unknown lines are not stored, so Mode A does not write them back. Mode A preserves the
  geometry and every field it models — not the file's text.
* **Writing an OBJ carries material *references*, not definitions.** Those live in
  a separate `.mtl`: call `write_material_lib` and save it under the name the
  `mtllib` line gives. A model whose libraries were loaded therefore does not
  compare equal to one re-read from Mode A output alone — the geometry does; the
  shading definitions were in the other file.
* **OBJ cannot clear a material, object or group.** `None` is expressible only
  before the first `usemtl`, `o` or `g`. A hand-built model that goes material →
  none → material reads back with the earlier material still in effect, which is
  what the format says; the same holds for objects and groups.
* **A material no face references is not written**, for the same reason: OBJ text
  has nowhere to put it.
* **Smoothing groups (`s`) are still unmodelled**, and `bind` ignores materials
  and groups entirely — this is geometry plus shading, not a scene graph.
* **Relative (negative) face indices are unsupported** — in the position field
  *and* in `vt`/`vn`, where the reference reader silently left them unbound.
  Refusing is the safer default: a dropped normal is exactly the quiet wrongness
  this crate was written to end.
* **`u32` index space.** More than 2^32 vertices in one file is not addressable.
* Non-ASCII bytes are decoded lossily; a byte that lands inside a numeric field
  becomes a `BadFloat` on that line.
* `NaN` and `inf` are accepted wherever `std`'s float parser accepts them, and
  written back in the same textual form.

## Feature flags

| Flag | Effect |
|---|---|
| `serde` | `Serialize`/`Deserialize` on the model types, the material types and `BoundMesh` |

The core crate has **no** dependencies — no `nom`, no `thiserror`, no `ahash`;
the parser is a `std` line scanner and the error type is hand-rolled.
`proptest` is a dev-dependency and never reaches a consumer.

## Licensing

MIT OR Apache-2.0.

`assets/samples/` carries its own provenance notes. The **Stanford bunny and
dragon are research/educational-licensed scan data (Turk/Levoy)**: they are in
the repository for the test suite and are `exclude`d from the published crate.
Tests that need them skip when the files are absent, so a registry checkout
builds and tests green. `1crn_ses_0.5A.obj` (a crambin solvent-excluded surface
generated by [molses](https://github.com/uvar-apps/molses) from the unrestricted
PDB entry 1CRN) ships with the crate and exercises the `vn`-binding path end to
end.
