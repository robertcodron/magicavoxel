//! Convert `.vox` files to glTF: `cargo run -p mv-format --example vox_to_gltf -- in.vox out.glb [scale]`
//! (`.glb` = binary, `.gltf` = JSON with embedded buffer).

use mv_format::{export_gltf, load_vox_file, GltfOptions};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (Some(input), Some(output)) = (args.first(), args.get(1)) else {
        eprintln!("usage: vox_to_gltf <in.vox> <out.glb|out.gltf> [scale]");
        std::process::exit(2);
    };
    let scale = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(1.0);
    let project = load_vox_file(input).unwrap_or_else(|e| {
        eprintln!("cannot read {input}: {e}");
        std::process::exit(1);
    });
    let opts = GltfOptions {
        scale,
        ..Default::default()
    };
    match export_gltf(&project, output, &opts) {
        Ok(doc) => println!(
            "{output}: {} nodes, {} meshes, {} materials, {} triangles",
            doc.nodes.len(),
            doc.meshes.len(),
            doc.materials.len(),
            doc.triangle_count()
        ),
        Err(e) => {
            eprintln!("export failed: {e}");
            std::process::exit(1);
        }
    }
}
