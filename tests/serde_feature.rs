//! The optional `serde` feature: it must add derives to the model types, the
//! material types and [`BoundMesh`], nothing else. Tested with trait bounds
//! rather than a JSON crate, because a format library would be a second
//! dependency to justify.

#![cfg(feature = "serde")]

use objserde::{
    BoundMesh, Channel, Corner, Face, LibraryStatus, Map, MapKind, MapOption, Material,
    MaterialLib, Obj, TexCoord, Triangle, UnknownTag, Vertex,
};

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
    assert_serde::<Material>();
    assert_serde::<MaterialLib>();
    assert_serde::<LibraryStatus>();
    assert_serde::<Map>();
    assert_serde::<MapKind>();
    assert_serde::<MapOption>();
    assert_serde::<Channel>();
    assert_serde::<UnknownTag>();
}

#[test]
fn serde_uses_the_core_types_without_the_feature() {
    // The same construction has to compile either way: the feature only adds
    // derives, it changes no shape.
    let obj = Obj {
        header: vec![" made by hand".into()],
        vertices: vec![Vertex::new([1.0, 2.0, 3.0])],
        normals: vec![[0.0, 0.0, 1.0]],
        tex_coords: vec![TexCoord::new(&[0.5, 0.5]).unwrap()],
        faces: vec![Face {
            corners: vec![Corner {
                v: 0,
                vt: Some(0),
                vn: Some(0),
            }],
            object: Some(0),
            group: Some(0),
            smooth_group: Some(0),
            ..Default::default()
        }],
        material_libs: vec![MaterialLib {
            path: "mats.mtl".into(),
            status: LibraryStatus::Unresolved,
        }],
        materials: vec![Material {
            name: "paint".into(),
            defined: true,
            diffuse: Some([0.1, 0.2, 0.3]),
            maps: vec![Map {
                kind: objserde::MapKind::Diffuse,
                path: "wood.png".into(),
                options: vec![objserde::MapOption::BumpScale(0.5)],
            }],
            ..Default::default()
        }],
        objects: vec!["protein".into()],
        groups: vec!["component_0".into()],
        smooth_groups: vec!["off".into()],
    };
    assert_eq!(obj.bind().triangle_count(), 0);
    assert_eq!(obj.object_of(&obj.faces[0]), Some("protein"));
    assert_eq!(obj.group_of(&obj.faces[0]), Some("component_0"));
    assert_eq!(obj.material_libs[0].status, LibraryStatus::Unresolved);
    assert_eq!(obj.materials[0].maps[0].kind, MapKind::Diffuse);
    assert_eq!(obj.faces[0].corners[0].vn, Some(0));
}
