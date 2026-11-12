# MagicaVoxel — Rust Port (`magicavoxel-rs`)

Réimplémentation native en **Rust** de **MagicaVoxel** : éditeur de modèles, éditeur de monde multi-objets, animation, console, Voxel Shaders, rendu **GPU Path Tracer `wgpu`** et import/export `.vox`, `.glb`, `.obj`, `.ply`, `.png`.

La documentation officielle d'origine (`ephtracy.github.io`) est conservée dans le dossier [`docs/`](file:///Users/robbie/Code/magicavoxel/docs/index.html) et la feuille de route dans [`ROADMAP.md`](file:///Users/robbie/Code/magicavoxel/ROADMAP.md).

## Architecture du Workspace Cargo

- [`crates/mv-core`](file:///Users/robbie/Code/magicavoxel/crates/mv-core/src/lib.rs) : Modèle 3D (`VoxelGrid` jusqu'à `256³`, `Palette` 256 couleurs, `Material` PBR, brushes et symétries, sélections, historique Undo/Redo compressé), graphe de scène ([`scene.rs`](file:///Users/robbie/Code/magicavoxel/crates/mv-core/src/scene.rs)), moteur de commandes console et Voxel Shaders `xs` ([`xs.rs`](file:///Users/robbie/Code/magicavoxel/crates/mv-core/src/xs.rs)).
- [`crates/mv-format`](file:///Users/robbie/Code/magicavoxel/crates/mv-format/src/lib.rs) : Lecture/écriture `.vox` (multi-modèles, graphe de scène `nTRN`/`nGRP`/`nSHP`, calques `LAYR`, frames d'animation), export **glTF 2.0** `.glb`/`.gltf` de la scène ([`gltf.rs`](file:///Users/robbie/Code/magicavoxel/crates/mv-format/src/gltf.rs)), `.obj` greedy meshing et `.ply` nuage de points de la scène, tranches `.png` du modèle édité.
- [`crates/mv-render`](file:///Users/robbie/Code/magicavoxel/crates/mv-render/src/lib.rs) :
  - **Vues Model / World :** ray-caster 3D DDA parallélisé (`rayon`), ambient occlusion, ombres, picking 3D exact ([`world.rs`](file:///Users/robbie/Code/magicavoxel/crates/mv-render/src/world.rs)).
  - **Vue Render :** path tracer GPU `wgpu` / WGSL ([`pathtrace.wgsl`](file:///Users/robbie/Code/magicavoxel/crates/mv-render/src/pathtrace.wgsl) & [`gpu_pathtracer.rs`](file:///Users/robbie/Code/magicavoxel/crates/mv-render/src/gpu_pathtracer.rs)) sur toute la scène : matériaux **Diffuse**, **Metal**, **Glass**, **Emit**, **Cloud**, Depth of Field, ACES.
  - **Voxel Shaders externes** compatibles MagicaVoxel `shader/*.txt` ([`voxel_shader.rs`](file:///Users/robbie/Code/magicavoxel/crates/mv-render/src/voxel_shader.rs)).
- [`crates/mv-app`](file:///Users/robbie/Code/magicavoxel/crates/mv-app/src/main.rs) : Application desktop `eframe` / `egui` : **Model Editor**, **World Editor** (`TAB` bascule Model ↔ World ; scene outline, calques, groupes, instances, union, déplacement, rotation, sol), timeline d'animation, panneau Voxel Shader, View Cube, glisser-déposer de `.vox`, sauvegarde automatique toutes les 25 modifications dans `cache/backup.vox`, confirmation des modifications non sauvegardées. Console : `F1`, capture de rendu : `F6`.
- [`crates/mv-mcp`](file:///Users/robbie/Code/magicavoxel/crates/mv-mcp/src/main.rs) : Serveur MCP pour assistants IA (voir la section suivante).

## Serveur MCP (création de voxel art par IA)

Le binaire `mv-mcp` est un serveur **MCP** (Model Context Protocol) qui fonctionne sur stdio (JSON-RPC 2.0, un message par ligne). Il permet à un assistant IA de construire des objets et des scènes voxel avec le même moteur que l'application (même format `.vox`, mêmes palettes, matériaux, graphe de scène et rendus). Il suffit de demander un dessin à une IA pour obtenir un fichier `.vox` ou `.glb`.

### Compilation et enregistrement

```bash
cargo build --release -p mv-mcp
./target/release/mv-mcp            # --help et --version disponibles
```

Avec Claude Code :

```bash
claude mcp add magicavoxel -- /chemin/vers/target/release/mv-mcp
```

Pour un client générique :

```json
{"mcpServers": {"magicavoxel": {"command": "/chemin/vers/mv-mcp"}}}
```

La variable d'environnement optionnelle `MV_SHADER_DIR` indique un dossier de Voxel Shaders externes. Les dossiers `./shader` et `./shaders` sont aussi explorés.

### Conventions à connaître

- Axes **Z-up** : X = droite, Y = profondeur, Z = haut ; le sol est `z = 0`.
- Canevas de 1 à 256 voxels par axe.
- Index de palette : `0` = vide, `1..=255` = couleurs.
- Chaque outil d'édition est **une seule étape d'annulation** (`history`).
- `voxel_batch` est **atomique** : tout ou rien.
- Les coordonnées hors du canevas sont tronquées (clipping).

### Outils (27)

**Projet**

| Outil | Rôle |
| :--- | :--- |
| `project_new` | Crée un projet vide (taille, nom). |
| `project_open` | Ouvre un projet `.vox`, Qubicle `.qb` ou heightmap de terrain `.png`. |
| `project_save` | Enregistre le projet en `.vox`. |
| `project_export` | Exporte en `vox`, `glb`/`gltf`, `obj`, `ply`, `slice_png`, `qb`, `heightmap_png`, `marching_cubes_obj` ou `marching_cubes_ply`. |
| `project_info` | Donne un aperçu JSON du projet, de la scène et de l'animation. |
| `console_command` | Exécute n'importe quelle commande console (`size`, `mir`, `xs wave`…). |
| `history` | Annule ou refait (`scope` : `model` ou `world`). |

**Voxels**

| Outil | Rôle |
| :--- | :--- |
| `voxel_set` | Pose des voxels individuels. |
| `voxel_box` | Dessine une boîte, pleine ou creuse. |
| `voxel_sphere` | Dessine une sphère ou un ellipsoïde. |
| `voxel_cylinder` | Dessine un cylindre, un cône ou un tronc de cône. |
| `voxel_line` | Dessine un segment 3D, éventuellement épais. |
| `voxel_batch` | Enchaîne plusieurs opérations atomiquement (recommandé). |
| `voxel_get` | Lit les voxels non vides d'une région (JSON, 5000 max). |
| `voxel_clear` | Vide le modèle édité (taille conservée). |
| `voxel_shader` | Lance un Voxel Shader (`xs`) : `list` ou `run`. |

**Palette & matériaux**

| Outil | Rôle |
| :--- | :--- |
| `palette_manage` | Lit ou modifie les couleurs de la palette, et choisit la couleur active. |
| `material_manage` | Lit, règle ou réinitialise le matériau PBR d'un index (`diffuse`, `metal`, `glass`, `emit`, `cloud`, `blend`). |

**Scène & animation**

| Outil | Rôle |
| :--- | :--- |
| `scene_manage` | Crée, déplace, tourne, groupe, instancie, fusionne et masque les objets du monde. |
| `layer_manage` | Liste ou modifie les 8 calques de la scène. |
| `animation` | Gère les frames de l'objet édité (info, sélection, ajout, suppression). |

**Rendu**

| Outil | Rôle |
| :--- | :--- |
| `render_preview` | Rend une image PNG (`model`, `world` ou `pathtracer`) pour vérifier le résultat. |
| `camera` | Lit ou règle la caméra utilisée par `render_preview` (`auto_frame` désactivé). |

**Cozy & Procédural**

| Outil | Rôle |
| :--- | :--- |
| `smart_brush` | Tracé spline et biomes : `curve_wall`, `smart_arch`, `fence`, `paint_path`, `grow_ivy`, `pond`, `tree`. |
| `architecture_generate` | Éléments d'architecture procéduraux : `roof` (gabled, hipped, mansard, conical), `window`, `door`, `chimney`. |
| `light_manage` | Gestion des lumières et accessoires : `add`, `remove`, `clear`, `list`, `spawn_lantern`. |
| `gizmo_transform` | Transformations et grille : `extrude` (face/sélection), `translate` (magnétisme de grille), `snap_config`. |

### Exemple de workflow

1. `project_new` avec `size_x`, `size_y`, `size_z` (par exemple 20, 20, 20).
2. `palette_manage` pour définir quelques couleurs.
3. `voxel_batch` pour construire l'objet :

```json
{"operations": [
  {"op": "box", "min": [0, 0, 0], "max": [19, 19, 0], "color": 3},
  {"op": "cylinder", "center": [10, 10, 1], "axis": "z", "height": 10, "radius": 4, "color": 12},
  {"op": "sphere", "center": [10, 10, 14], "radius": 4, "color": 14}
]}
```

4. `render_preview` (`mode: "world"`, puis d'autres `yaw` pour voir les autres faces).
5. `project_export` avec `format: "glb"` pour le fichier final.

D'autres opérations batch existent : `pyramid`, `torus`, `line`, `set`, `replace_color`, `clear`, ainsi que les options `hollow` (et `wall_thickness` pour boîtes et sphères).

### Ressources et prompts

Ressources : `magicavoxel://project/info` et `magicavoxel://project/guide`. Prompt : `create_voxel_object` (argument `subject`).

### Conseils

- `render_preview` en mode `model` ou `world` est rapide et fidèle ; préférez `world` pour les contrôles rapides. Le mode `pathtracer` est physiquement réaliste mais rend actuellement une image un peu délavée (limitation connue).

### Tests

`cargo test -p mv-mcp` (tests unitaires et tests de protocole sur le binaire réel).

> **Règle de développement :** toute nouvelle fonctionnalité de l'éditeur visible par l'utilisateur doit aussi être accessible via le MCP, soit par un outil dédié, soit via `console_command` lorsqu'il s'agit d'une commande console (voir [`AGENTS.md`](file:///Users/robbie/Code/magicavoxel/AGENTS.md)).

## Lancer et tester

```bash
cargo run --release -p mv-app     # application
cargo test --workspace            # tests unitaires
```

Toolchain : `rustc 1.87.0`. Certaines dépendances sont épinglées dans `Cargo.toml` et `Cargo.lock` ; ne lancez jamais de `cargo update` global.
