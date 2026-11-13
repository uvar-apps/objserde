//! Round-trip and determinism properties: what the writers promise, tested as
//! properties rather than as one stored file.

mod common;

use objserde::{
    BoundMesh, Corner, Face, LibraryStatus, Material, MaterialLib, Obj, Precision, TexCoord,
    Vertex, WriteOptions, write_mesh,
};
use proptest::prelude::*;
use proptest::test_runner::Config;

/// A coordinate like the ones real files carry: exact zeros, long decimals,
/// big and small magnitudes.
fn coord() -> impl Strategy<Value = f64> {
    prop_oneof![
        Just(0.0f64),
        Just(-0.0f64),
        Just(1.0f64),
        Just(1.0 / 3.0),
        Just(1e-9f64),
        Just(1e15f64),
        any::<i32>().prop_map(|v| v as f64),
        any::<f64>().prop_map(|v| if v.is_finite() { v } else { 0.5 }),
        (any::<i32>(), -6i32..6i32).prop_map(|(m, e)| (m as f64) * 10f64.powi(e)),
    ]
}

fn triple() -> impl Strategy<Value = [f64; 3]> {
    (coord(), coord(), coord()).prop_map(|(x, y, z)| [x, y, z])
}

/// Index space `[0, upper)`; never generates the `u32::MAX` sentinel the model
/// reserves for "cannot be addressed".
fn index_below(upper: usize) -> impl Strategy<Value = u32> {
    0..(upper.max(1) as u32)
}

/// A model with at least one vertex (a model with none cannot be re-read, that
/// is `NoVertices`) and every reference inside its tables.
fn arb_obj() -> impl Strategy<Value = Obj> {
    (1usize..6, 0usize..5, 0usize..5).prop_flat_map(|(n_verts, n_normals, n_tex)| {
        let vertices =
            prop::collection::vec((triple(), prop::collection::vec(coord(), 0..4)), n_verts);
        let normals = prop::collection::vec(triple(), n_normals);
        let tex = prop::collection::vec(
            prop::collection::vec(coord(), 1..=TexCoord::MAX_COMPONENTS),
            n_tex,
        );
        // One to four faces; each has 4 or fewer corners referencing the
        // vertices this model actually has, plus an optional material switch in
        // front of it (`Some(n)` stands for `usemtl mat{n}`).
        let faces = prop::collection::vec(
            (
                prop::collection::vec(
                    (
                        index_below(n_verts),
                        prop::option::weighted(0.5, index_below(n_tex)),
                        prop::option::weighted(0.5, index_below(n_normals)),
                    ),
                    1..5,
                ),
                prop::option::weighted(0.4, 0u32..3),
            ),
            0..4,
        );
        // Library paths, including ones with spaces (which the writer must quote
        // so they read back as a single name).
        let lib_names = prop::collection::vec(
            prop_oneof![
                Just("materials.mtl".to_string()),
                Just("sub/dir/mats.mtl".to_string()),
                Just("my materials.mtl".to_string()),
                Just("Mat's Way.mtl".to_string()),
            ],
            0..3,
        );
        (vertices, normals, tex, faces, lib_names).prop_map(
            move |(vertices, normals, tex, faces, lib_names)| {
                // Materials in OBJ are *state*: `usemtl` sets it for every face
                // that follows. Generate it that way so the model a writer can
                // actually reproduce is the model being tested — and so the
                // material table ends up in first-use order, as a re-parse
                // builds it.
                let mut materials: Vec<Material> = Vec::new();
                let mut current: Option<u32> = None;
                let mut out_faces = Vec::with_capacity(faces.len());
                for (corners, switch) in faces {
                    if let Some(want) = switch {
                        let name = format!("mat{want}");
                        let index = match materials.iter().position(|m| m.name == name) {
                            Some(i) => i,
                            None => {
                                materials.push(Material::referenced(name));
                                materials.len() - 1
                            }
                        };
                        current = Some(index as u32);
                    }
                    out_faces.push(Face {
                        corners: corners
                            .into_iter()
                            .map(|(v, vt, vn)| Corner { v, vt, vn })
                            .collect(),
                        material: current,
                    });
                }
                Obj {
                    vertices: vertices
                        .into_iter()
                        .map(|(position, extra)| Vertex { position, extra })
                        .collect(),
                    normals,
                    tex_coords: tex
                        .iter()
                        .filter_map(|values| TexCoord::new(values))
                        .collect(),
                    faces: out_faces,
                    material_libs: lib_names
                        .into_iter()
                        .map(|path| MaterialLib {
                            path,
                            status: LibraryStatus::Unresolved,
                        })
                        .collect(),
                    materials,
                }
            },
        )
    })
}

proptest! {
    #![proptest_config(Config { cases: 512, ..Config::default() })]

    /// The headline promise of layer 1: write then read gives the same model.
    #[test]
    fn parse_of_to_string_is_the_same_model(obj in arb_obj()) {
        let text = obj.to_string();
        let again = objserde::parse(&text).expect("a written model must read back");
        prop_assert_eq!(&again, &obj);
    }

    /// Bit identity, not merely equality: `-0.0` comes back as `-0.0`.
    #[test]
    fn round_trip_preserves_float_bits(obj in arb_obj()) {
        let again = objserde::parse(&obj.to_string()).unwrap();
        for (a, b) in obj.vertices.iter().zip(&again.vertices) {
            for k in 0..3 {
                prop_assert_eq!(
                    a.position[k].to_bits(),
                    b.position[k].to_bits(),
                    "position component {}", k
                );
            }
            prop_assert_eq!(a.extra.len(), b.extra.len(), "trailing field count");
            for (x, y) in a.extra.iter().zip(&b.extra) {
                prop_assert_eq!(x.to_bits(), y.to_bits(), "trailing vertex field");
            }
        }
        for (a, b) in obj.normals.iter().zip(&again.normals) {
            for k in 0..3 {
                prop_assert_eq!(
                a[k].to_bits(),
                b[k].to_bits(),
                "vn component {}", k
            );
            }
        }
        for (a, b) in obj.tex_coords.iter().zip(&again.tex_coords) {
            prop_assert_eq!(a.arity, b.arity, "vt arity");
            for k in 0..a.as_slice().len() {
                prop_assert_eq!(a.components[k].to_bits(), b.components[k].to_bits());
            }
        }
    }

    /// Writing is a pure function of the model, and reading a written model
    /// writes the identical bytes again.
    #[test]
    fn to_string_is_deterministic(obj in arb_obj()) {
        prop_assert_eq!(obj.to_string(), obj.to_string());
        let again = objserde::parse(&obj.to_string()).unwrap();
        prop_assert_eq!(again.to_string(), obj.to_string());
    }

    /// The binding layer survives the canonical writer: a smooth mesh written
    /// out and re-read carries its own normals.
    #[test]
    fn written_mesh_binds_back_to_its_normals(mesh in arb_smooth_mesh()) {
        let options = WriteOptions { precision: Precision::Shortest, header: Vec::new() };
        let text = write_mesh(&mesh, &options);
        let rebound = objserde::parse(&text).unwrap().bind();
        for (i, (want, got)) in mesh
            .unit_normals
            .iter()
            .zip(&rebound.unit_normals)
            .enumerate()
        {
            for k in 0..3 {
                prop_assert!(
                    (want[k] - got[k]).abs() < 1e-12,
                    "vertex {i} component {k}: wrote {want:?}, read back {got:?}"
                );
            }
        }
        prop_assert_eq!(&rebound.triangles, &mesh.triangles);
    }

    /// Mode A keeps the model even when the file it produced is re-bound.
    #[test]
    fn geometry_survives_the_round_trip(obj in arb_obj()) {
        let again = objserde::parse(&obj.to_string()).unwrap();
        prop_assert_eq!(obj.bind().triangles, again.bind().triangles);
        prop_assert_eq!(obj.bind().positions, again.bind().positions);
    }
}

/// Smooth meshes with outward analytic normals, the shape `molses` generates.
fn arb_smooth_mesh() -> impl Strategy<Value = BoundMesh> {
    (3usize..12, 3usize..12, 0.5f64..25.0).prop_map(|(nu, nv, radius)| uv_sphere(radius, nu, nv))
}

/// Closed UV sphere with outward analytic normals — the same construction the
/// meshview self-tests use, so the two crates probe the same geometry.
pub fn uv_sphere(radius: f64, nu: usize, nv: usize) -> BoundMesh {
    let count = (nv + 1) * nu;
    let mut positions = Vec::with_capacity(count);
    let mut unit_normals = Vec::with_capacity(count);
    for j in 0..=nv {
        let theta = std::f64::consts::PI * j as f64 / nv as f64;
        let (st, ct) = (theta.sin(), theta.cos());
        for i in 0..nu {
            let phi = 2.0 * std::f64::consts::PI * i as f64 / nu as f64;
            let n = [st * phi.cos(), st * phi.sin(), ct];
            unit_normals.push(n);
            positions.push([radius * n[0], radius * n[1], radius * n[2]]);
        }
    }
    let mut triangles = Vec::with_capacity(nv * nu * 2);
    for j in 0..nv {
        for i in 0..nu {
            let a = j * nu + i;
            let b = j * nu + (i + 1) % nu;
            let c = (j + 1) * nu + i;
            let d = (j + 1) * nu + (i + 1) % nu;
            triangles.push([a as u32, c as u32, b as u32]);
            triangles.push([b as u32, c as u32, d as u32]);
        }
    }
    BoundMesh {
        positions,
        unit_normals,
        triangles,
    }
}

/// The promise on real files, not just generated ones: Mode A round-trips each
/// sample exactly, and the bound mesh is unchanged by the trip.
#[test]
fn each_sample_round_trips_through_mode_a() {
    for name in [
        "1crn_ses_0.5A.obj",
        "stanford-bunny.obj",
        "xyzrgb_dragon.obj",
    ] {
        if !common::have(name) {
            eprintln!("skipping {name}: not shipped with the crates.io package");
            continue;
        }
        let obj = objserde::parse_file(common::sample(name)).unwrap();
        let text = obj.to_string();
        let again = objserde::parse(&text).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(again, obj, "{name} did not survive a Mode A round trip");
        assert_eq!(again.to_string(), text, "{name}: second pass differs");
        let before = obj.bind();
        let after = again.bind();
        assert_eq!(before.triangles, after.triangles, "{name}: triangles moved");
        assert_eq!(before.positions, after.positions, "{name}: positions moved");
        assert_eq!(
            before.unit_normals, after.unit_normals,
            "{name}: bound normals moved"
        );
    }
}
