//! The binding contract: the two checks that exist because the wrong answer
//! once produced a false comparison (`selftest.rs` rules 12 and 13), plus the
//! rest of the uvar rule set written out as tests.

mod common;

use objserde::parse;

/// Flat-shaded exports index `vn` differently from `v`; binding by arrival order
/// silently gives every vertex some nearby normal, which looks right and is not.
#[test]
fn vn_binds_by_index_not_arrival_order() {
    let perm = "\
v 0 0 0
v 1 0 0
v 0 1 0
vn 0 0 1
vn 0 1 0
vn 1 0 0
f 1//3 2//1 3//2
";
    let mesh = parse(perm).unwrap().bind();
    let expected: [[f64; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]];
    for (i, (got, want)) in mesh.unit_normals.iter().zip(&expected).enumerate() {
        for k in 0..3 {
            assert!(
                (got[k] - want[k]).abs() < 1e-9,
                "vertex {i}: got {mesh:?} expected {expected:?}",
            );
        }
    }
}

/// A corner's middle field is a texture index. Reading it as a normal is the
/// other way this goes wrong.
#[test]
fn vt_corners_fall_back_to_geometric_normals() {
    let with_vt = "\
v 0 0 0
v 1 0 0
v 0 1 0
vt 0 0
f 1/1 2/1 3/1
";
    let obj = parse(with_vt).unwrap();
    assert_eq!(obj.faces[0].corners[0].vt, Some(0));
    assert_eq!(obj.faces[0].corners[0].vn, None);

    let mesh = obj.bind();
    // The triangle lies in z = 0, so a correct geometric fallback has |nz| = 1.
    for n in &mesh.unit_normals {
        assert!(
            n[2].abs() > 1.0 - 1e-9,
            "expected the geometric fallback, got {:?}",
            mesh.unit_normals[0]
        );
    }
}

/// `a/b` where b is a texture index must never be confused with `a//c`, even
/// when both appear in the same file.
#[test]
fn mixed_vt_and_vn_corners_bind_only_the_vn() {
    let src = "\
v 0 0 0
v 1 0 0
v 0 1 0
vt 3 3
vn 0 0 1
vn 0 1 0
f 1/1 2/1 3//2
";
    let mesh = parse(src).unwrap().bind();
    // Only the third corner carries a vn, so only vertex 2 is bound; the other
    // two take the geometric normal of the z = 0 triangle.
    assert!(
        (mesh.unit_normals[2][1] - 1.0).abs() < 1e-9,
        "vertex 2 should hold vn 2: {:?}",
        mesh.unit_normals[2]
    );
    for i in [0, 1] {
        assert!(
            mesh.unit_normals[i][2].abs() > 1.0 - 1e-9,
            "vertex {i} should be geometric: {:?}",
            mesh.unit_normals[i]
        );
    }
}

/// First corner to reference a position wins, and a later face does not
/// overwrite it.
#[test]
fn first_referencing_corner_wins() {
    let src = "\
v 0 0 0
v 1 0 0
v 0 1 0
v 0 0 1
vn 0 0 1
vn 0 1 0
f 1//1 2//1 3//1
f 1//2 2//2 4//2
";
    let mesh = parse(src).unwrap().bind();
    assert!(
        (mesh.unit_normals[0][2] - 1.0).abs() < 1e-9,
        "vertex 0 keeps the normal of the first face that referenced it: {:?}",
        mesh.unit_normals[0]
    );
    // Vertex 3 only appears in the second face, so it takes vn 2.
    assert!(
        (mesh.unit_normals[3][1] - 1.0).abs() < 1e-9,
        "{:?}",
        mesh.unit_normals[3]
    );
}

/// A `vn` past the end of the table is unbound, not an error: the file still
/// loads and the vertex falls back to geometry.
#[test]
fn out_of_range_vn_leaves_the_corner_unbound() {
    let src = "\
v 0 0 0
v 1 0 0
v 0 1 0
vn 0 0 1
f 1//1 2//9 3//1
";
    let obj = parse(src).unwrap();
    assert_eq!(obj.faces[0].corners[1].vn, Some(8), "index kept as written");
    let mesh = obj.bind();
    assert!(
        mesh.unit_normals[1][2].abs() > 1.0 - 1e-9,
        "unbound vertex takes the geometric normal: {:?}",
        mesh.unit_normals[1]
    );
}

/// Exported normals are not always unit. They are rescaled, and the direction is
/// kept exactly.
#[test]
fn non_unit_vn_is_rescaled_not_replaced() {
    let src = "\
v 0 0 0
v 1 0 0
v 0 1 0
vn 0 0 7.5
f 1//1 2//1 3//1
";
    let obj = parse(src).unwrap();
    assert_eq!(
        obj.normals[0],
        [0.0, 0.0, 7.5],
        "the raw table keeps the value as written"
    );
    let mesh = obj.bind();
    assert_eq!(mesh.unit_normals[0], [0.0, 0.0, 1.0]);
}

/// A zero-length `vn` stays zero: binding must not invent a direction.
#[test]
fn zero_vn_stays_exactly_zero() {
    let src = "\
v 0 0 0
v 1 0 0
v 0 1 0
vn 0 0 0
f 1//1 2//1 3//1
";
    let mesh = parse(src).unwrap().bind();
    for (i, n) in mesh.unit_normals.iter().enumerate() {
        assert_eq!(*n, [0.0, 0.0, 0.0], "vertex {i} grew a direction: {n:?}");
        assert!(mesh.is_zero_normal(i));
    }
}

/// A vertex no face references has no orientation: zero, not a guess.
#[test]
fn unreferenced_vertex_has_zero_normal() {
    let src = "\
v 0 0 0
v 1 0 0
v 0 1 0
v 9 9 9
f 1 2 3
";
    let mesh = parse(src).unwrap().bind();
    assert_eq!(mesh.unit_normals.len(), 4, "one normal per position");
    assert_eq!(mesh.unit_normals[3], [0.0, 0.0, 0.0]);
    assert_eq!(mesh.positions[3], [9.0, 9.0, 9.0], "position still there");
}

/// Degenerate faces produce no triangles but are not dropped from the model.
#[test]
fn thin_faces_produce_no_triangles_but_stay_in_the_model() {
    let src = "\
v 0 0 0
v 1 0 0
v 0 1 0
f 1 2
f 1
f 1 2 3
";
    let obj = parse(src).unwrap();
    assert_eq!(obj.faces.len(), 3);
    assert_eq!(obj.faces[0].corners.len(), 2);
    assert_eq!(obj.faces[1].corners.len(), 1);
    assert_eq!(obj.triangulate().len(), 1);
    assert_eq!(obj.bind().triangle_count(), 1);
}

/// A polygon fans into `n - 2` triangles with corner 0 in every triangle.
#[test]
fn fan_triangulation_shape() {
    let src = "\
v 0 0 0
v 1 0 0
v 1 1 0
v 0 1 0
v 2 2 0
f 1 2 3 4 5
";
    let obj = parse(src).unwrap();
    let tris = obj.triangulate();
    assert_eq!(tris.len(), 3);
    assert_eq!(
        tris.iter()
            .map(|t| t.corners.map(|c| c.v))
            .collect::<Vec<_>>(),
        vec![[0, 1, 2], [0, 2, 3], [0, 3, 4]]
    );
}

/// The geometric fallback is an area-weighted sum, so a vertex shared by one big
/// and one tiny face points along the big face.
#[test]
fn geometric_fallback_is_area_weighted() {
    let src = "\
v 0 0 0
v 1 0 0
v 0 1 0
v 0 0 100
v 100 0 0
f 1 2 3
f 1 5 4
";
    let mesh = parse(src).unwrap().bind();
    // Face (1,5,4) has ~50 times the area of (1,2,3), so its normal dominates at
    // the shared vertex 0.
    let n = mesh.unit_normals[0];
    assert!(n[1].abs() > 0.99, "dominant face points along +y: {n:?}");
}
