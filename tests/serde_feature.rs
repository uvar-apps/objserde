//! The optional `serde` feature: it must add derives to the model types and to
//! [`BoundMesh`], nothing else. Tested with trait bounds rather than a JSON
//! crate, because a format library would be a second dependency to justify.

#![cfg(feature = "serde")]

use objserde::{BoundMesh, Corner, Face, Obj, TexCoord, Triangle, Vertex};

fn assert_serde<T: serde::Serialize + serde::de::DeserializeOwned>() {}

#[test]
fn every_model_type_is_serde_round_trippable() {
    assert_serde::<Obj>();
    assert_serde::<Vertex>();
    assert_serde::<TexCoord>();
    assert_serde::<Face>();
    assert_serde::<Corner>();
    assert_serde::<Triangle>();
    assert_serde::<BoundMesh>();
}

#[test]
fn serde_uses_the_core_types_without_the_feature() {
    // The same construction has to compile either way: the feature only adds
    // derives, it changes no shape.
    let obj = Obj {
        vertices: vec![Vertex::new([1.0, 2.0, 3.0])],
        normals: vec![[0.0, 0.0, 1.0]],
        tex_coords: vec![TexCoord::new(&[0.5, 0.5]).unwrap()],
        faces: vec![Face {
            corners: vec![Corner {
                v: 0,
                vt: Some(0),
                vn: Some(0),
            }],
        }],
    };
    assert_eq!(obj.bind().triangle_count(), 0);
    assert_eq!(obj.faces[0].corners[0].vn, Some(0));
}
