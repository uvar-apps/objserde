//! Shared helpers for the ported meshview contracts.
//!
//! The geometric-normal reference here is written independently of `src/bind.rs`
//! on purpose: a test that reuses the implementation proves nothing. Ported from
//! `meshview/tests/loader.rs` and `tests/common/mod.rs`.

#![allow(dead_code)]

use objserde::BoundMesh;

/// Path to a bundled sample asset.
pub fn sample(name: &str) -> String {
    format!("{}/assets/samples/{name}", env!("CARGO_MANIFEST_DIR"))
}

/// True when a sample asset exists. The crates.io package omits the Stanford
/// scan models (research/educational license), so their tests skip there.
pub fn have(name: &str) -> bool {
    std::path::Path::new(&sample(name)).exists()
}

/// Vertex indices actually referenced by at least one triangle corner.
///
/// Unreferenced vertices are excluded from normal assertions: no face asked for
/// a normal there, so a zero is the documented answer, not a bug.
pub fn referenced(mesh: &BoundMesh) -> Vec<usize> {
    let mut seen = vec![false; mesh.vertex_count()];
    for tri in &mesh.triangles {
        for &i in tri {
            seen[i as usize] = true;
        }
    }
    seen.iter()
        .enumerate()
        .filter(|(_, seen)| **seen)
        .map(|(i, _)| i)
        .collect()
}

/// Area-weighted vertex normals, recomputed from the bound mesh: each triangle
/// adds its unnormalized `(b − a) × (c − a)` to all three corners in triangle
/// order, then unit-length is imposed with the same zero guard the loader uses.
pub fn geometric(mesh: &BoundMesh) -> Vec<[f64; 3]> {
    let mut out = vec![[0.0f64; 3]; mesh.vertex_count()];
    for tri in &mesh.triangles {
        let a = mesh.positions[tri[0] as usize];
        let b = mesh.positions[tri[1] as usize];
        let c = mesh.positions[tri[2] as usize];
        let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let cr = [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ];
        for &i in tri {
            for (slot, x) in out[i as usize].iter_mut().zip(cr) {
                *slot += x;
            }
        }
    }
    for n in &mut out {
        let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        if len > 1e-12 {
            for c in n {
                *c /= len;
            }
        }
    }
    out
}

pub fn length(n: &[f64; 3]) -> f64 {
    (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt()
}

/// Read the `vn` table straight out of a file, bypassing the crate.
pub fn vertex_normals_from_file(path: &str) -> Vec<[f64; 3]> {
    let text = std::fs::read_to_string(path).expect("sample readable");
    let mut vns = Vec::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("vn ") {
            let mut it = rest.split_whitespace();
            vns.push([
                it.next().unwrap().parse().unwrap(),
                it.next().unwrap().parse().unwrap(),
                it.next().unwrap().parse().unwrap(),
            ]);
        }
    }
    vns
}

/// Path to a file under `tests/fixtures/mtl/`.
pub fn mtl_fixture(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/mtl")
        .join(name)
}
