# MagicaVoxel — Rust Port (`magicavoxel-rs`)

Réimplémentation native en **Rust** de **MagicaVoxel** (Model Editor + Console + 3D DDA Ray-Caster + **GPU Path Tracer `wgpu` PBR** + `.vox` / `.obj` / `.ply` / `slice` I/O).

La documentation officielle d'origine (`ephtracy.github.io`) est conservée dans le dossier [`docs/`](file:///Users/robbie/Code/magicavoxel/docs/index.html).

## Architecture du Workspace Cargo

- [`crates/mv-core`](file:///Users/robbie/Code/magicavoxel/crates/mv-core/src/lib.rs) : Noyau de modélisation 3D (`VoxelGrid` jusqu'à `256³`, `Palette` 256 couleurs, `Material` PBR, `SelectionMask`, `BrushSettings` avec symétries `Mirror` et `Axis`, pile `History` Undo/Redo compressée RLE, et moteur de commandes console + Voxel Shaders `xs`).
- [`crates/mv-format`](file:///Users/robbie/Code/magicavoxel/crates/mv-format/src/lib.rs) : Lecture/écriture binaire du format `.vox` (`SIZE`, `XYZI`, `RGBA`, `MATL`), export `.obj` avec **Greedy Meshing**, export nuage de points `.ply` et export tranches PNG 2D (`slice`).
- [`crates/mv-render`](file:///Users/robbie/Code/magicavoxel/crates/mv-render/src/lib.rs) :
  - **Vue Model Editor :** Ray-Caster 3D DDA parallélisé (`rayon`) avec Ambient Occlusion analytique par voxel, ombres portées directionnelles, arêtes, grille au sol, boîte englobante et **Ray-Picking 3D exact** (voxel + normale).
  - **Vue Renderer (Path Tracer GPU) :** Compute Shader **WGSL / `wgpu`** ([`pathtrace.wgsl`](file:///Users/robbie/Code/magicavoxel/crates/mv-render/src/pathtrace.wgsl) & [`gpu_pathtracer.rs`](file:///Users/robbie/Code/magicavoxel/crates/mv-render/src/gpu_pathtracer.rs)) avec accumulation progressive Monte-Carlo, rebonds multiples, matériaux PBR (**Diffuse**, **Metal GGX**, **Glass** avec réfraction IOR, **Emit HDR**, **Cloud** volumétrique), **Depth of Field** (DOF au clic) et **ACES Filmic Tone Mapping**.
- [`crates/mv-app`](file:///Users/robbie/Code/magicavoxel/crates/mv-app/src/main.rs) : Application Desktop native (`eframe` / `egui`) intégrant le sélecteur **Model / Render**, la palette 256 couleurs, l'éditeur de matériaux PBR, les outils de sculpture, la console interactive (`TAB` / `F1`) et l'export de rendus `.png` (`F6`).

## Lancer l'application

```bash
cargo run --release -p mv-app
```

## Lancer les tests unitaires

```bash
cargo test --workspace
```
