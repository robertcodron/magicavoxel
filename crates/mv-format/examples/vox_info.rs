//! Print the scene graph of `.vox` files and check that they survive a save/load roundtrip.
//!
//! Usage: `cargo run -p mv-format --example vox_info -- file1.vox [file2.vox ...]`

use mv_core::{NodeKind, Scene};
use mv_format::{from_vox_bytes, to_vox_bytes};

fn print_node(scene: &Scene, id: usize, depth: usize) {
    let n = scene.node(id).expect("live node");
    let t = n.transform.translation;
    let kind = match &n.kind {
        NodeKind::Group { children } => format!("group ({} children)", children.len()),
        NodeKind::Shape { model_id } => {
            let d = scene.models[*model_id].dimensions();
            format!("shape model #{model_id} {}x{}x{}", d.x, d.y, d.z)
        }
    };
    println!(
        "{:indent$}- [{id}] {kind} t=({}, {}, {}) r={} layer={}{}{}",
        "",
        t.x,
        t.y,
        t.z,
        n.transform.rotation.to_vox_byte(),
        n.layer_id,
        n.name
            .as_ref()
            .map(|s| format!(" name={s:?}"))
            .unwrap_or_default(),
        if n.hidden { " hidden" } else { "" },
        indent = depth * 2
    );
    for &c in n.children() {
        print_node(scene, c, depth + 1);
    }
}

fn main() {
    let mut failures = 0;
    for path in std::env::args().skip(1) {
        println!("== {path}");
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) => {
                println!("   read error: {e}");
                failures += 1;
                continue;
            }
        };
        let project = match from_vox_bytes(&bytes) {
            Ok(p) => p,
            Err(e) => {
                println!("   decode error: {e}");
                failures += 1;
                continue;
            }
        };
        let scene = &project.scene;
        let shapes = scene.shapes_under(scene.root());
        println!(
            "   {} models, {} nodes, {} shapes, active model #{}, world bounds {:?}",
            scene.models.len(),
            scene.node_count(),
            shapes.len(),
            project.active_model,
            scene.world_bounds(scene.root())
        );
        if scene.node_count() <= 40 {
            print_node(scene, scene.root(), 1);
        }

        // Roundtrip: world placement and voxel content of every shape must be preserved.
        let again = from_vox_bytes(&to_vox_bytes(&project)).expect("re-decode");
        let shapes2 = again.scene.shapes_under(again.scene.root());
        let same = shapes.len() == shapes2.len()
            && shapes.iter().zip(&shapes2).all(|(&a, &b)| {
                let (na, nb) = (scene.node(a).unwrap(), again.scene.node(b).unwrap());
                scene.world_transform(a) == again.scene.world_transform(b)
                    && scene.is_visible(a) == again.scene.is_visible(b)
                    && na.name == nb.name
                    && scene.models[na.model_id().unwrap()]
                        == again.scene.models[nb.model_id().unwrap()]
            })
            && again.palette.colors == project.palette.colors
            && again.materials == project.materials;
        println!("   roundtrip: {}", if same { "OK" } else { "MISMATCH" });
        if !same {
            failures += 1;
        }
    }
    std::process::exit(if failures == 0 { 0 } else { 1 });
}
