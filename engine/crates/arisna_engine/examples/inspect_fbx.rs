use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use arisna_engine::{
    bevy::{
        asset::{AssetPlugin, LoadState},
        image::Image,
        mesh::skinning::SkinnedMeshInverseBindposes,
        prelude::*,
        world_serialization::WorldAsset,
    },
    Fbx, FbxLoaderSettings, FbxPlugin,
};

fn main() {
    let path = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .expect("usage: inspect_fbx <absolute-file.fbx>");
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .expect("FBX path must end with a UTF-8 file name")
        .to_owned();
    let asset_root = path
        .parent()
        .expect("FBX path must have a parent directory")
        .to_string_lossy()
        .into_owned();

    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin {
            file_path: asset_root,
            ..default()
        },
    ))
    .init_asset::<Mesh>()
    .init_asset::<StandardMaterial>()
    .init_asset::<Image>()
    .init_asset::<WorldAsset>()
    .init_asset::<SkinnedMeshInverseBindposes>()
    .add_plugins(FbxPlugin);

    let handle: Handle<Fbx> = app
        .world()
        .resource::<AssetServer>()
        .load_builder()
        .with_settings(|settings: &mut FbxLoaderSettings| {
            settings.load_auxiliary_geometry = false;
        })
        .load::<Fbx>(file_name.clone());
    let started = Instant::now();
    loop {
        app.update();
        match app.world().resource::<AssetServer>().load_state(&handle) {
            LoadState::Loaded => break,
            LoadState::Failed(error) => panic!("failed to load {file_name}: {error}"),
            _ if started.elapsed() > Duration::from_secs(180) => {
                panic!("timed out loading {file_name}")
            }
            _ => std::thread::sleep(Duration::from_millis(10)),
        }
    }

    let world = app.world();
    let fbx_assets = world.resource::<Assets<Fbx>>();
    let fbx = fbx_assets
        .get(&handle)
        .expect("FBX asset missing after successful load");
    let mesh_assets = world.resource::<Assets<Mesh>>();
    let vertices: usize = mesh_assets
        .iter()
        .map(|(_, mesh)| mesh.count_vertices())
        .sum();

    println!("FBX={}", path.display());
    println!("SCENES={}", fbx.scenes.len());
    println!("MESHES={}", fbx.meshes.len());
    println!("MATERIALS={}", fbx.materials.len());
    println!("NODES={}", fbx.nodes.len());
    println!("SKINS={}", fbx.skins.len());
    println!("VERTICES={vertices}");
    let mut mesh_names: Vec<_> = fbx
        .named_meshes
        .keys()
        .map(|name| name.to_string())
        .collect();
    mesh_names.sort();
    println!("NAMED_MESHES={}", mesh_names.join("|"));
    let node_assets = world.resource::<Assets<bevy_ufbx::FbxNode>>();
    let mut node_names: Vec<_> = fbx
        .named_nodes
        .iter()
        .filter_map(|(name, handle)| {
            node_assets
                .get(handle)
                .map(|node| format!("{}:visible={}", name, node.visible))
        })
        .collect();
    node_names.sort();
    println!("NAMED_NODES={}", node_names.join("|"));
    for (name, mesh_handle) in &fbx.named_meshes {
        let Some(mesh) = mesh_assets.get(mesh_handle) else {
            continue;
        };
        let Some(bevy::mesh::VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            continue;
        };
        let mut min = [f32::INFINITY; 3];
        let mut max = [f32::NEG_INFINITY; 3];
        for position in positions {
            for axis in 0..3 {
                min[axis] = min[axis].min(position[axis]);
                max[axis] = max[axis].max(position[axis]);
            }
        }
        println!(
            "MESH_BOUNDS name={} min={:?} max={:?} size={:?}",
            name,
            min,
            max,
            [max[0] - min[0], max[1] - min[1], max[2] - min[2]]
        );
    }
    println!("LOAD_MS={}", started.elapsed().as_millis());
}
