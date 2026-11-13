//! Ported loader contracts from `meshview/tests/loader.rs`, plus the four
//! fixture checks from `meshview/tests/selftest.rs`, against the new two-layer
//! API (`parse` for the raw model, `bind` for the shaded mesh).
//!
//! Sample-dependent tests skip when the file is absent: the two Stanford scan
//! models are research/educational licensed and stay out of the published crate,
//! while the crambin SES mesh ships. A registry checkout therefore has 1CRN but
//! not bunny or dragon, and the suite has to be green either way.

mod common;

use objserde::{ObjError, parse, parse_file};

// ---------------------------------------------------------------------------
// The three samples: exact counts, exact normal behaviour.
// ---------------------------------------------------------------------------

#[test]
fn bunny_loads_with_exact_counts_and_geometric_normals() {
    if !common::have("stanford-bunny.obj") {
        eprintln!("skipping: stanford-bunny.obj is not shipped with the crates.io package");
        return;
    }
    let obj = parse_file(common::sample("stanford-bunny.obj")).unwrap();
    assert_eq!(obj.vertices.len(), 35_947);
    assert_eq!(obj.triangulate().len(), 69_451);
    assert!(
        obj.normals.is_empty(),
        "the bunny carries no vn table at all"
    );

    let mesh = obj.bind();
    assert_eq!(mesh.vertex_count(), 35_947);
    assert_eq!(mesh.triangle_count(), 69_451);

    // No vn anywhere: every referenced vertex must carry its own geometric
    // normal, unit length.
    let geo = common::geometric(&mesh);
    let mut min_len = f64::MAX;
    for i in common::referenced(&mesh) {
        let n = mesh.unit_normals[i];
        for k in 0..3 {
            assert!(
                (n[k] - geo[i][k]).abs() < 1e-9,
                "vertex {i}: bound normal {n:?} != normalized geometric {:?}",
                geo[i]
            );
        }
        min_len = min_len.min(common::length(&n));
    }
    assert!(
        (min_len - 1.0).abs() < 1e-9,
        "smallest referenced |n| is {min_len}, expected unit"
    );
}

#[test]
fn dragon_loads_with_exact_counts_and_keeps_zero_area_vertices() {
    if !common::have("xyzrgb_dragon.obj") {
        eprintln!("skipping: xyzrgb_dragon.obj is not shipped with the crates.io package");
        return;
    }
    let obj = parse_file(common::sample("xyzrgb_dragon.obj")).unwrap();
    assert_eq!(obj.vertices.len(), 125_066);
    assert_eq!(obj.triangulate().len(), 249_882);
    assert!(obj.normals.is_empty());

    let mesh = obj.bind();
    // Scan data: folded, zero-area corners are real, and their normal stays
    // exactly zero rather than being invented or turned into NaN.
    let geo = common::geometric(&mesh);
    let mut zeros = 0usize;
    for i in common::referenced(&mesh) {
        let n = mesh.unit_normals[i];
        for k in 0..3 {
            assert!(
                (n[k] - geo[i][k]).abs() < 1e-9,
                "dragon vertex {i}: {n:?} vs geometric {:?}",
                geo[i]
            ); 
        }
        let len = common::length(&n);
        assert!(
            (len - 1.0).abs() < 1e-9 || len == 0.0,
            "dragon vertex {i} |n| {len} is neither unit nor zero"
        );
        if len == 0.0 {
            zeros += 1;
        }
    }
    assert!(
        zeros > 0,
        "the dragon is expected to have genuine zero-area vertices; found {zeros}"
    );
    eprintln!("dragon referenced vertices with zero geometric normal: {zeros}");
}

#[test]
fn crambin_keeps_the_files_own_vn_normals_renormalized() {
    let path = common::sample("1crn_ses_0.5A.obj");
    let obj = parse_file(&path).unwrap();
    assert_eq!(obj.vertices.len(), 37_930);
    assert_eq!(obj.triangulate().len(), 75_852);
    assert_eq!(obj.normals.len(), 37_930);

    // `f i//i` over a watertight mesh: vertex i binds to vn line i, and the
    // stored value survives modulo the unit rescale.
    let vns = common::vertex_normals_from_file(&path);
    assert_eq!(vns.len(), obj.vertices.len());

    let mesh = obj.bind();
    let mut worst_renorm = 0.0f64;
    for (i, (n, v)) in mesh.unit_normals.iter().zip(&vns).enumerate() {
        let len = common::length(v);
        worst_renorm = worst_renorm.max((len - 1.0).abs());
        for k in 0..3 {
            let want = v[k] / len.max(1e-12);
            assert!(
                (n[k] - want).abs() < 1e-12,
                "vertex {i}: stored vn {v:?} not kept (got {n:?})"
            );
        }
    }
    // The mesh ships nearly-unit normals; if this ever moves, the
    // re-normalization is doing real work worth calling out.
    eprintln!("worst |vn| - 1 in the file: {worst_renorm:.3e}");
}

/// The `f i//i` form must preserve each corner's own `vn` index rather than
/// collapsing to the first one seen.
#[test]
fn f_slash_slash_i_form_binds_each_normals_own_index() {
    let path = common::sample("1crn_ses_0.5A.obj");
    let obj = parse_file(&path).unwrap();
    // Spot-check the raw model: corner k of face k carries the vn index that
    // the file wrote, not a shared one.
    let face = &obj.faces[10];
    for (k, corner) in face.corners.iter().enumerate() {
        assert_eq!(corner.vn, Some(corner.v), "face corner {k} lost its vn index");
        assert_eq!(corner.vt, None, "vn-only corners must not gain a vt");
    }
}

// ---------------------------------------------------------------------------
// Corner-form matrix (loader.rs `vertex_indices_and_corner_forms_all_parse`).
// ---------------------------------------------------------------------------

#[test]
fn vertex_indices_and_corner_forms_all_parse() {
    let src = "\
v 0 0 0 1.0 0.2 0.3
vn 0 0 1 7
vt 0.5 0.5
f 1 1 1
f 1//1 1//1 1//1
f 1/1/1 1/1/1 1/1/1
f 1/1 1/1 1/1
";
    let obj = parse(src).unwrap();
    assert_eq!(obj.vertices.len(), 1);
    assert_eq!(obj.faces.len(), 4);
    assert_eq!(obj.triangulate().len(), 4);

    // The trailing vertex fields and the extra vn field are modelled, not lost.
    assert_eq!(obj.vertices[0].extra, vec![1.0, 0.2, 0.3]);
    // Corner shapes survive: bare, //vn, /vt/vn, /vt.
    assert_eq!(obj.faces[0].corners[0].vt, None);
    assert_eq!(obj.faces[0].corners[0].vn, None);
    assert_eq!(obj.faces[1].corners[0].vn, Some(0));
    assert_eq!(obj.faces[2].corners[0].vt, Some(0));
    assert_eq!(obj.faces[2].corners[0].vn, Some(0));
    assert_eq!(obj.faces[3].corners[0].vt, Some(0));
    assert_eq!(obj.faces[3].corners[0].vn, None);

    let mesh = obj.bind();
    let n = mesh.unit_normals[0];
    assert!(
        (n[2] - 1.0).abs() < 1e-9,
        "normal bound from the corner forms: {n:?}"
    );
}

// ---------------------------------------------------------------------------
// Error contracts.
// ---------------------------------------------------------------------------

#[test]
fn empty_file_with_comments_errors() {
    let err = parse("# nothing\nvt 0 0\nvnotavertex\n").unwrap_err();
    assert!(matches!(err, ObjError::NoVertices), "{err}");
    assert!(err.to_string().contains("no vertices"), "{err}");
}

#[test]
fn malformed_number_names_the_line() {
    let src = "v 0 0 0\nv 1 x 0\n";
    let err = parse(src).unwrap_err();
    match err {
        ObjError::BadFloat { line, got } => {
            assert_eq!(line, 2);
            assert_eq!(got, "x");
        }
        other => panic!("expected BadFloat, got {other}"),
    }
    // The message names the line, which is what makes it actionable.
    assert!(
        parse("v 0 0 0\nv 1 x 0\n")
            .unwrap_err()
            .to_string()
            .contains("line 2")
    );

    let src = "v 0 0 0\nf 1 2 zzz\n";
    let err = parse(src).unwrap_err();
    match err {
        ObjError::BadFloat { line, .. } => assert_eq!(line, 2),
        other => panic!("expected BadFloat, got {other}"),
    }
}

#[test]
fn truncated_coordinate_names_the_line() {
    let err = parse("v 0 0 0\nv 1 2\n").unwrap_err();
    match err {
        ObjError::BadFloat { line, got } => {
            assert_eq!(line, 2);
            assert_eq!(got, "<missing field>");
        }
        other => panic!("expected BadFloat, got {other}"),
    }
}

#[test]
fn face_index_out_of_range_errors() {
    let src = "v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 9\n";
    let err = parse(src).unwrap_err();
    match err {
        ObjError::IndexOutOfRange {
            line, value, count,
        } => {
            assert_eq!(line, 4);
            assert_eq!(value, 9);
            assert_eq!(count, 3);
        }
        other => panic!("expected IndexOutOfRange, got {other}"),
    }
    let msg = parse(src).unwrap_err().to_string();
    assert!(msg.contains("line 4") && msg.contains("out of range"), "{msg}");
}

#[test]
fn every_out_of_range_face_reports_its_own_line() {
    // Validating against the finished table (not during the read) is what lets
    // the first bad face win without the later ones hiding it.
    let src = "v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\nf 1 2 7\nf 4 1 1\n";
    let err = parse(src).unwrap_err();
    match err {
        ObjError::IndexOutOfRange { line, value, count } => {
            assert_eq!(line, 5, "the first offending face should report its own line");
            assert_eq!(value, 7);
            assert_eq!(count, 3);
        }
        other => panic!("expected IndexOutOfRange, got {other}"),
    }
}

#[test]
fn negative_face_index_rejected_with_clear_error() {
    let src = "v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 -1\n";
    let err = parse(src).unwrap_err();
    match err {
        ObjError::NegativeIndex { line, value } => {
            assert_eq!(line, 4);
            assert_eq!(value, -1);
        }
        other => panic!("expected NegativeIndex, got {other}"),
    }
    let msg = parse(src).unwrap_err().to_string();
    assert!(msg.contains("relative") && msg.contains("not supported"), "{msg}");
}

#[test]
fn zero_position_index_is_rejected_not_wrapped() {
    let src = "v 0 0 0\nv 1 0 0\nv 0 1 0\nf 0 1 2\n";
    let err = parse(src).unwrap_err();
    match err {
        ObjError::NegativeIndex { line, value } => {
            assert_eq!(line, 4);
            assert_eq!(value, 0);
        }
        other => panic!("expected NegativeIndex, got {other}"),
    }
}

// ---------------------------------------------------------------------------
// The four fixture checks ported from `selftest.rs` (8-11).
// ---------------------------------------------------------------------------

/// Explicit `v//vn` indices plus a polygon face.
const FIXTURE: &str = "\
# fixture
v 0 0 0
v 1 0 0
v 1 1 0
v 0 1 0
v 0 1 1
vn 0 0 1
vn 0 -1 0
vn 1 0 0
f 1//1 2//1 3//1 4//1
f 4//2 5//2 1//2
";

#[test]
fn fixture_reads_five_vertices() {
    let obj = parse(FIXTURE).unwrap();
    assert_eq!(obj.vertices.len(), 5);
}

#[test]
fn fixture_fans_quad_into_triangles() {
    let obj = parse(FIXTURE).unwrap();
    // The quad keeps all four corners; fanning gives 2, plus the triangle: 3.
    assert_eq!(obj.faces[0].corners.len(), 4);
    assert_eq!(obj.bind().triangle_count(), 3);
}

#[test]
fn fixture_keeps_one_normal_per_vertex() {
    let mesh = parse(FIXTURE).unwrap().bind();
    assert_eq!(mesh.unit_normals.len(), 5);
}

#[test]
fn fixture_normals_are_unit_length() {
    let mesh = parse(FIXTURE).unwrap().bind();
    for n in &mesh.unit_normals {
        let len = common::length(n);
        assert!((len - 1.0).abs() < 1e-9, "norm {len}");
    }
}
