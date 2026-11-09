# AGENTS.md — Guide Technique pour Agents IA & Claude Code

Ce document contient tout le contexte architectural, les conventions techniques et l'état d'avancement nécessaires pour reprendre et poursuivre le développement du portage Rust de **MagicaVoxel** (`magicavoxel-rs`).

---

## 1. Vue d'Ensemble & Contexte du Projet

- **Objectif :** Réimplémenter **MagicaVoxel** (application Desktop native macOS / Windows / Linux) en **Rust**.
- **Origine du dépôt :** Ce dépôt était initialement un clone de `ephtracy.github.io` (le site officiel de MagicaVoxel contenant toute la documentation fonctionnelle, la liste des commandes console, les raccourcis clavier et l'historique des fonctionnalités jusqu'à la version `0.99.7.2`).
- **Spécifications de référence conservées dans `docs/` :**
  - [`docs/mv_main.html`](file:///Users/robbie/Code/magicavoxel/docs/mv_main.html) : Changelog complet détaillant toutes les fonctionnalités de MagicaVoxel.
  - [`docs/mv_commands.html`](file:///Users/robbie/Code/magicavoxel/docs/mv_commands.html) : Spécification officielle des commandes de la console (`TAB` / `F1`).
  - [`docs/mv_controls.html`](file:///Users/robbie/Code/magicavoxel/docs/mv_controls.html) : Spécification officielle de tous les raccourcis clavier, modes de caméra, brushes (`V/F/B/L/C/P`) et symétries.
  - [`docs/mv_renderer.html`](file:///Users/robbie/Code/magicavoxel/docs/mv_renderer.html) : Spécification du Path Tracer et des formats supportés.
- **Usage par des IA (MCP) :** les assistants IA peuvent créer des objets et des scènes voxel via le serveur MCP `mv-mcp` (crate `crates/mv-mcp`, voir §3.5).

> [!IMPORTANT]
> **Règle de parité MCP — toute nouvelle fonctionnalité doit être accessible par le MCP.**
> Dès qu'une fonctionnalité utilisateur est ajoutée ou modifiée dans l'éditeur (outil, commande, panneau, import/export, rendu, animation, scène, matériaux…), elle doit être utilisable par une IA dans le **même** changement, sinon le travail est incomplet :
> 1. **Commande console** (`mv-core/src/command.rs`) : déjà accessible via l'outil MCP `console_command`, rien d'autre à faire (mais documenter la commande).
> 2. **Fonction du noyau sans commande console** (nouvelle opération sur la grille, la scène, la palette, les matériaux, un export…) : ajouter un **outil MCP dédié** (ou une nouvelle `action` d'un outil existant) dans `crates/mv-mcp/src/tools/`, avec ses tests.
> 3. **Fonction purement visuelle de l'UI** (disposition des panneaux, View Cube…) : exemptée, mais si elle change l'état du projet, de la caméra ou du rendu, exposer cet état (outil `camera`, `project_info`…).
> 4. Mettre à jour le tableau des outils du [`README.md`](file:///Users/robbie/Code/magicavoxel/README.md) et la liste de la §3.5.
> Les outils MCP appellent le **noyau** (`mv-core`, `mv-format`, `mv-render`), jamais `mv-app` : la logique d'une fonctionnalité doit donc vivre dans le noyau pour servir l'UI et le MCP.

- **Feuille de route :** Voir [`ROADMAP.md`](file:///Users/robbie/Code/magicavoxel/ROADMAP.md) pour le suivi détaillé des phases terminées (Phases 1, 2, 3, 4, 5, 6) et des phases restantes (Phases 7 et 8).

---

## 2. Commandes Essentielles & Contrainte Toolchain (`rustc 1.87.0`)

```bash
# Compiler et lancer l'application Desktop (toujours privilégier --release pour la fluidité 3D)
cargo run --release -p mv-app

# Compiler le serveur MCP pour les IA (stdio, voir §3.5)
cargo build --release -p mv-mcp

# Exécuter l'ensemble des tests unitaires du workspace (mv-core, mv-format, mv-render, mv-mcp, mv-app)
cargo test --workspace

# Vérifier la compilation rapide sans lancer de binaire
cargo check --workspace
```

> [!IMPORTANT]
> **Compatibilité MSRV (`rustc 1.87.0`) :**
> L'environnement utilise `rustc 1.87.0`. Certaines versions récentes de crates transitives (`image >= 0.25.10`, `icu_* >= 2.4.0`, `uuid >= 1.29.1`) exigent `rustc 1.88+`.
> - Dans `Cargo.toml`, `image` est volontairement épinglé à `=0.25.5`.
> - Dans `Cargo.lock`, `idna_adapter` est épinglé à `1.2.0` (`icu_* 1.5.x`) et `uuid` à `1.16.0`.
> - **Ne jamais exécuter un `cargo update` global** sans conserver ces versions verrouillées dans `Cargo.lock`.

---

## 3. Architecture du Workspace Cargo

Le projet est découpé en 4 crates dans `crates/` :

### 3.1 `crates/mv-core` (Modèle de données, Édition & Console)
Aucune dépendance graphique ni I/O disque : 100 % pur Rust et math (`glam`).
- [`src/voxel.rs`](file:///Users/robbie/Code/magicavoxel/crates/mv-core/src/voxel.rs) :
  - `VoxelGrid` : Grille 3D jusqu'à `256x256x256` (`MAX_GRID_DIM = 256`).
  - **Convention d'axes MagicaVoxel (`Z`-up)** :
    - `X` : droite (`0..size_x`)
    - `Y` : profondeur / avant-arrière (`0..size_y`)
    - `Z` : hauteur / vertical (`0..size_z`, le sol est le plan `z = 0`)
    - Indexation linéaire : `index = x + y * size_x + z * size_x * size_y`.
  - Opérations volumiques : `resize`, `shrink`, `clear` (`zero`), `full`, `fill_solid` (`fill`), `invert` (`inv`), `flip`, `loop_shift`, `scale`, `rot90`, `rotate_degrees`, `repeat`, `mirror_symmetry`, `diagonal_symmetry`, `dilate`, `erode`, `flood`, `generate_noise`, `rand_paint`, `generate_maze`, `shear`, `sample_monument`.
- [`src/palette.rs`](file:///Users/robbie/Code/magicavoxel/crates/mv-core/src/palette.rs) :
  - `Palette` : 256 entrées `Rgba` (`colors[0]` est transparent/vide, `colors[1..=255]` sont les 255 couleurs utilisables).
  - Supporte `generate_bw`, `generate_mac`, `fill_from_index`, `mask_unused`, `swap`, `duplicate`, `fill_gradient`, `sort_by_spec` (`+-hsvrgb`).
- [`src/material.rs`](file:///Users/robbie/Code/magicavoxel/crates/mv-core/src/material.rs) :
  - `MaterialType` (`Diffuse`, `Metal`, `Glass`, `Emit`, `Blend`, `Cloud`) et `Material` (propriétés PBR associées à chacun des 256 index de palette).
- [`src/selection.rs`](file:///Users/robbie/Code/magicavoxel/crates/mv-core/src/selection.rs) :
  - `SelectionMask` (masque booléen 3D de la taille de `VoxelGrid`) et `VoxelClipboard` (`copy`, `cut`, `paste`).
- [`src/brush.rs`](file:///Users/robbie/Code/magicavoxel/crates/mv-core/src/brush.rs) :
  - `BrushAction` (`Attach`, `Erase`, `Paint`, `Select`), `BrushMode` (`Voxel`, `Face`, `Box`, `Line`, `Center`, `Pattern`), `VoxelSubShape` (`Cube`, `Sphere`).
  - `BrushSettings::compute_shape_voxels` calcule les voxels de base, `expand_symmetries` applique `Mirror (X/Y/Z)` et `Axis (X/Y/Z)`, et `apply` modifie la `VoxelGrid` et la `SelectionMask`.
- [`src/history.rs`](file:///Users/robbie/Code/magicavoxel/crates/mv-core/src/history.rs) :
  - Pile Undo/Redo de 100 niveaux utilisant un encodage RLE (`CompressedSnapshot`) sur la grille de voxels ; chaque snapshot inclut aussi la palette et les 256 matériaux.
  - Côté UI, une modification de swatch/matériau (drag de slider, clic, color picker) produit **une seule** entrée d'undo (`swatch_edit_in_progress`).
- [`src/command.rs`](file:///Users/robbie/Code/magicavoxel/crates/mv-core/src/command.rs) :
  - Parser et exécuteur de toutes les commandes console de `docs/mv_commands.html` + Voxel Shaders intégrés (`xs wave`, `xs sphere`, `xs torus`, `xs cylinder`, `xs gyroid`, `xs pyramid`, `xs checker`, `xs life`).
- [`src/scene.rs`](file:///Users/robbie/Code/magicavoxel/crates/mv-core/src/scene.rs) :
  - Graphe de scène du World Editor : `Scene { models, layers, nodes }`, `SceneNode` (un `nTRN` + enfant groupe/shape), `Transform`, `Rotation` (octet `_r` du `.vox`).
  - Placement monde d'un voxel `v` : `rotation * (v - size/2) + translation` (division entière), composé avec les transforms des groupes parents.
  - Opérations World Editor : instances / duplicats (`Ctrl+T`), `move_to_ground` (`G`), `rotate90`, `union` (`U`), `compact_models`.
  - **Animation** : `NodeKind::Shape { models }` = un modèle par frame (jamais vide) ; `node.model_id()` = frame 0, `scene.shape_model(id)` = frame courante (`scene.current_frame`, hors `SceneStructure`). Toute requête géométrique utilise la frame courante. `extra_transform_keys` = keyframes `nTRN` préservées mais non évaluées.
- [`src/xs.rs`](file:///Users/robbie/Code/magicavoxel/crates/mv-core/src/xs.rs) :
  - `parse_xs` (`-n`, `-prev`, `-cur`) et `EditorProject::run_xs(inv, |grid, step| ...)` : exécution commune aux shaders intégrés et externes, sur copie, validée en une seule étape d'undo. Un nom hors `BUILTIN_SHADERS` renvoie `CommandOutcome::ExternalShader`, exécuté par l'app.
- [`src/project.rs`](file:///Users/robbie/Code/magicavoxel/crates/mv-core/src/project.rs) :
  - `active_shape` = shape dont la timeline du Model Editor parcourt les frames (`open_shape`, `set_frame`, `add_frame`, `delete_frame`, `refresh_active_model`). Les historiques d'undo sont **par modèle** (`model_histories`).
  - `EditorProject.grid` est le modèle **en cours d'édition**, extrait de `scene.models[active_model]` (qui peut être périmé pendant l'édition) : lire les modèles via `project.model(id)`, ou appeler `commit_active_model()` avant de manipuler `scene.models`. `set_active_model` réinitialise l'undo et la sélection (par modèle).
  - `EditorProject::new_empty` part de matériaux neutres (`Material::default_table()`) ; seuls `Default::default()` (projet de démo) a des presets émissif/métal/verre.
  - `EditorProject` : agrège `VoxelGrid`, `Scene`, `Palette`, `[Material; 256]`, `SelectionMask`, `VoxelClipboard`, `BrushSettings`, `History` et l'historique de la console.

### 3.2 `crates/mv-format` (Sérialisation `.vox` & Exports 3D/2D)
- [`src/lib.rs`](file:///Users/robbie/Code/magicavoxel/crates/mv-format/src/lib.rs) :
  - `to_vox_bytes` / `from_vox_bytes` / `save_vox_file` / `load_vox_file` : lecture/écriture binaire du format `.vox` (v150) avec chunks `MAIN`, `SIZE`, `XYZI`, `RGBA`, `MATL`, et scène multi-modèles (`nTRN`, `nGRP`, `nSHP`, `LAYR`) dans [`src/scene_io.rs`](file:///Users/robbie/Code/magicavoxel/crates/mv-format/src/scene_io.rs).
  - Outil de diagnostic : `cargo run -p mv-format --example vox_info -- fichier.vox` affiche le graphe de scène et vérifie le roundtrip.
  - `MATL` : flottants écrits en représentation exacte (`{}`), `_ior` stocké en **`ior - 1`** (convention MagicaVoxel), ids `1..=255` seulement. Tous les champs de `Material` sont lus/écrits (`_weight`, `_rough`, `_spec`, `_ior`, `_att`, `_flux`, `_emit`, `_ldr`, `_metal`, `_alpha`/`_trans`, `_d` pour la densité). Seuls les matériaux différents du défaut sont écrits.
  - **Subtilité du format `.vox` pour `RGBA` :** Dans la spécification `.vox` d'Ephtracy, l'entrée `0` du chunk `RGBA` stocke la couleur d'index `1` de la palette (et ainsi de suite de `0..254` -> `1..255`). Ce décalage est déjà géré dans `mv-format`.
  - [`src/mesh.rs`](file:///Users/robbie/Code/magicavoxel/crates/mv-format/src/mesh.rs) : `greedy_quads` (greedy meshing 2D par tranche, quads CCW vus de l'extérieur, axes MagicaVoxel), partagé par les exports.
  - `export_obj_greedy` / `export_ply_point_cloud` : **scène visible** (un `o` par shape, positions monde via `placed.rs`) ; conversion d'axes OBJ **`(x, y, z) -> (x, z, -y)`** ; ordre des sommets inversé pour les rotations miroir (`Rotation::is_mirror`).
  - [`src/gltf.rs`](file:///Users/robbie/Code/magicavoxel/crates/mv-format/src/gltf.rs) : export glTF 2.0 de **toute la scène** (`to_gltf` construit un document typé testable, puis `to_glb_bytes` / `to_gltf_json`). Matrices de nœuds `[R | t + n(R)]` (même convention que le rendu monde ; ces décalages se composent exactement le long de la hiérarchie).
  - `export_ply_point_cloud` : export `.ply` des voxels de surface visibles.
  - `export_slice_png` : export d'une bande PNG 2D de taille `(size_x, size_y * size_z)`.

### 3.3 `crates/mv-render` (Ray-Caster DDA CPU & Path Tracer GPU `wgpu`)
- [`src/lib.rs`](file:///Users/robbie/Code/magicavoxel/crates/mv-render/src/lib.rs) :
  - `OrbitCamera` : caméra orbitale en coordonnées sphériques `Z`-up (`yaw_deg`, `pitch_deg`, `distance`, `target`, `fov_deg`, 10 slots sauvegardables).
  - `dda_raycast_grid` & `pick_voxel` : traversée de grille 3D Amanatides & Woo (retourne le voxel exact `IVec3`, la normale de la face touchée `IVec3` et la distance `t`).
  - [`src/voxel_shader.rs`](file:///Users/robbie/Code/magicavoxel/crates/mv-render/src/voxel_shader.rs) : voxel shaders MagicaVoxel (GLSL `shader/*.txt`) exécutés sur GPU (`VoxelShaderEngine`, cache de pipelines, erreurs de compilation via error scopes). `build_compute_source` enveloppe la source ; `v` = centre du voxel (+0.5), `voxel()` arrondit à l'entier inférieur. Test de compatibilité : `XS_DIR=dossier cargo test -p mv-render xs_folder_compat -- --ignored --nocapture`.
  - [`src/world.rs`](file:///Users/robbie/Code/magicavoxel/crates/mv-render/src/world.rs) : `WorldView` (shapes visibles placées dans le monde), `render_world_rgba`, `pick_world`. Chaque shape est tracée dans son espace modèle (`world = R·p + offset`, avec +1 sur les axes inversés pour aligner les cellules).
  - `render_viewport_rgba` : rendu temps réel pour le **Model Editor** (parallélisé sur CPU avec `rayon`, incluant Ambient Occlusion analytique par coin de voxel, arêtes, grille `z=0`, cadre englobant, ombres portées et surbrillance de sélection/aperçu brush).
- [`src/gpu_pathtracer.rs`](file:///Users/robbie/Code/magicavoxel/crates/mv-render/src/gpu_pathtracer.rs) & [`src/pathtrace.wgsl`](file:///Users/robbie/Code/magicavoxel/crates/mv-render/src/pathtrace.wgsl) :
  - `PathTracerEngine` : pipeline Compute `wgpu` exécutant `pathtrace.wgsl` par workgroups de `8x8`.
  - **Toute la scène est rendue** (frame courante, objets/calques cachés exclus) via une structure à deux niveaux : `pack_scene` concatène les grilles des modèles visibles (packées 4 voxels par `u32`, chaque modèle aligné sur un mot, un seul exemplaire par modèle partagé) et écrit un `GpuInstance` par objet (matrice inverse, décalage, AABB monde, taille, adresse). `trace_scene` (WGSL) teste l'AABB de chaque instance puis `trace_instance` fait le DDA en espace local. Les données viennent de `WorldView` (même convention de placement que le viewport World).
  - `VoxelGrid::revision()` : identifiant unique global du contenu, renouvelé par **chaque** méthode `&mut self` (et par toute nouvelle grille). Tout nouveau mutateur de `VoxelGrid` doit le renouveler (`self.revision = next_grid_revision()`).
  - Tout changement de caméra, de paramètres de rendu (`PathTracerSettings`), de voxels (via `revision()`, sans hacher la grille) ou de matériaux est détecté par hachage (`compute_scene_hash` / `compute_camera_and_settings_hash`) et remet automatiquement `current_samples` à `0`.
  - Inclut un fallback CPU automatique si aucun adaptateur GPU n'est disponible (ex: CI headless).

### 3.4 `crates/mv-app` (Application Desktop `eframe` / `egui`)
- Organisation des modules (`src/`) :
  - `main.rs` : point d'entrée `eframe` uniquement.
  - `app.rs` : `MagicaVoxelApp` (état), `ViewportMode`, ouverture/sauvegarde/export, et `update()` qui appelle les panneaux dans l'ordre.
  - `shortcuts.rs` : `handle_global_shortcuts`.
  - `ui/top_bar.rs`, `ui/console.rs`, `ui/left_panel.rs` (palette, matériaux, brush), `ui/right_panel.rs` : un panneau egui par fichier.
  - `viewport.rs` : viewport central (contrôles caméra communs, puis Model Editor, World Editor ou path tracer).
  - `world_editor.rs` : état du World Editor (`WorldEditorState` : sélection = enfants directs du groupe courant `group`), opérations de scène, raccourcis, interaction viewport.
  - `ui/world_panel.rs` : Scene Outline, propriétés, Layers.
  - `ui/timeline.rs` : timeline d'animation (barre du bas) et lecture (`tick_animation`, `goto_frame`).
  - `shaders.rs` + `ui/shader_panel.rs` : découverte des `shader/*.txt`, moteur GPU créé à la demande, panneau Voxel Shader.
  - `view_cube.rs` : View Cube (faces projetées avec la base caméra ; `view_cube_faces` sert aussi aux tests).
  - `unsaved.rs` : dialogue modal « modifications non sauvegardées » (`request_open_path`, `handle_close_request` + `ViewportCommand::CancelClose`, `resolve_pending`). Utiliser `request_open_path` (pas `open_vox_path`) pour toute ouverture déclenchée par l'utilisateur.
  - `backup.rs` : auto-backup toutes les `BACKUP_EVERY` (25) modifications dans `cache/backup.vox` ; `open_vox_path` (dialogue et glisser-déposer) réinitialise le compteur.
  - `tests.rs` : tests d'intégration **headless** (`Harness` : vraies frames egui + événements clavier/souris simulés, `MagicaVoxelApp::with_project` + `frame()`). `world_snapshot` (ignoré) rend un `.vox` en PNG : `VOX_IN=a.vox PNG_OUT=a.png cargo test -p mv-app world_snapshot -- --ignored`.
- Conventions World Editor :
  - **Undo** : `finish_world_frame` compare `scene.structure()` en début/fin de frame et pousse un snapshot si changement (une étape par geste). Les opérations `world_*` n'enregistrent donc **jamais** l'undo elles-mêmes ; mettre `world.skip_record = true` pour un changement qui ne doit pas être annulable (undo/redo, ouverture de fichier).
  - `TAB` est intercepté dans `raw_input_hook` (sinon egui l'utilise pour la navigation au clavier) ; il reste au widget focalisé (console).
  - Ne jamais appeler une méthode de `ctx` qui écrit (ex. `set_pixels_per_point`) dans une closure `ctx.input(|i| ..)` : verrou egui → blocage. Différer (`pending_ui_scale`).
- Fonctionnement :
  - Deux modes de vue commutables dans la barre supérieure :
    - `ViewportMode::ModelEditor` : sculpture interactive au clic/glisser (`LButton`), pipette de couleur (`Alt+LButton`), focus caméra (`X+LButton`), panneau Brushes & Transformations.
    - `ViewportMode::PathTracer` : rendu progressif PBR GPU (`wgpu`), clic gauche sur un voxel pour régler la distance de focus **DOF**, `Alt+LButton` pour inspecter le matériau d'un voxel, `Ctrl+C`/`Ctrl+V`/`Ctrl+R` pour copier/coller/réinitialiser un matériau, et bouton **📷 Snap (`F6`)** pour exporter le rendu en PNG.

---

### 3.5 `crates/mv-mcp` (Serveur MCP pour les IA)
Serveur **MCP** (*Model Context Protocol*) sur stdio : JSON-RPC 2.0, un message par ligne (`mv-mcp --help` pour l'enregistrer dans un client). **Aucune dépendance externe** (JSON, base64 et protocole faits maison pour rester compatible `rustc 1.87.0`).
- [`src/session.rs`](file:///Users/robbie/Code/magicavoxel/crates/mv-mcp/src/session.rs) : `McpSession` (un `EditorProject` + caméra + moteurs de rendu/shaders créés à la demande) et `handle_jsonrpc` (`initialize`, `ping`, `tools/list`, `tools/call`, `resources/*`, `prompts/*`). Un échec d'outil renvoie `isError: true`, jamais une erreur de protocole.
- [`src/json.rs`](file:///Users/robbie/Code/magicavoxel/crates/mv-mcp/src/json.rs) : `JsonValue` (parseur, sérialiseur, `base64_encode`).
- [`src/primitives.rs`](file:///Users/robbie/Code/magicavoxel/crates/mv-mcp/src/primitives.rs) : formes (`draw_box`, `draw_ellipsoid`, `draw_cylinder`, `draw_pyramid`, `draw_torus`, `draw_line`, `replace_color`) et `execute_batch_op` (opérations de `voxel_batch`). **Toutes les boucles sont bornées au canevas** (coût ≤ 256³ quelle que soit la taille demandée) et les nombres au-delà de `MAX_COORD` sont refusés : ne pas réintroduire de boucle sur la forme demandée sans la clipper.
- [`src/tools/`](file:///Users/robbie/Code/magicavoxel/crates/mv-mcp/src/tools/mod.rs) : un module par groupe d'outils, chacun exposant `definitions()` (descripteurs MCP + JSON Schema) et `dispatch(session, nom, args) -> Option<ToolResult>` :
  - `project.rs` : `project_new/open/save/export/info`, `console_command`, `history`.
  - `voxel.rs` : `voxel_set/box/sphere/cylinder/line/batch/get/clear`, `palette_manage`, `material_manage`.
  - `scene.rs` : `scene_manage`, `layer_manage`, `animation`, `voxel_shader`.
  - `render.rs` : `render_preview` (PNG en base64 : modes `model`, `world`, `pathtracer`), `camera`.
- **Conventions des outils :** chaque appel réussi = **une seule** étape d'undo (modèle : `record_undo()` ; scène : `record_world_undo()`) ; les opérations travaillent sur une copie et ne s'appliquent que si tout réussit (**atomicité**) ; valider les entrées et renvoyer `Err(String)` clair (jamais de panic sur du JSON invalide) ; coordonnées MagicaVoxel `Z`-up ; les descriptions d'outils sont lues par les IA : y décrire précisément les champs.
- **Ajouter un outil :** `definitions()` + branche dans `dispatch()` du module concerné, puis tests (`McpSession::new()` + `dispatch_tool_call`), puis README. Les noms d'outils doivent rester uniques (test `definitions_*`).
- **Tests :** `cargo test -p mv-mcp` (unitaires + `tests/protocol.rs` qui lance le vrai binaire). Test de bout en bout manuel : lancer `target/release/mv-mcp` et dialoguer en JSON-RPC (voir `tests/protocol.rs` pour des exemples).

## 4. Guide pour la Suite du Développement (Phases 5 & 7)

Lorsque tu reprends le projet, voici les deux chantiers principaux décrits dans [`ROADMAP.md`](file:///Users/robbie/Code/magicavoxel/ROADMAP.md) :

### Chantier A — Phase 5 : *World Editor* & Graphe de Scène Multi-Objets (✅ Terminé à 100 %)
> Terminé : graphe de scène complet (`nTRN`, `nGRP`, `nSHP`, `LAYR`), instances et duplicates récursifs de groupes, Gizmos 3D avec axes projetés et plane drag, surface snapping (`Ctrl+LButton`), accélération spatiale BVH (`WorldBvh`), bibliothèque de patterns procéduraux, parité MCP (`scene_manage`).

1. **Créer `crates/mv-core/src/scene.rs`** :
   - Définir `SceneLayer` (8 calques avec `name`, `color`, `hidden`).
   - Définir `SceneObject` (ou un graphe `Node` avec `TransformNode`, `GroupNode`, `ShapeNode`) contenant `id`, `name`, `model_id: usize`, `translation: IVec3`, `rotation`, `layer_id: usize`, `hidden: bool`, `selected: bool`.
   - Permettre à plusieurs `SceneObject` de partager le même `model_id` (instanciation / *Reference*) et implémenter `convert_reference_to_duplicate` (`Ctrl+T`).
   - Implémenter les opérations du World Editor (`docs/mv_controls.html` lignes 188-242) :
     - Déplacement d'objets, alignement au sol `G` (`translation.z = 0`), rotation 90°, Union booléenne `U` de plusieurs objets.
2. **Étendre `crates/mv-format/src/lib.rs`** :
   - Lire et écrire plusieurs paires `SIZE` + `XYZI` ainsi que les chunks de graphe de scène `nTRN`, `nGRP`, `nSHP` et `LAYR`.
3. **Étendre `crates/mv-render` & `crates/mv-app`** :
   - Ajouter un mode `ViewportMode::WorldEditor` (bascule via `TAB` entre `ModelEditor` et `WorldEditor`).
   - Afficher les panneaux **Scene Outline** et **Layer Manager** à droite.

### Chantier B — Phase 7 : Animation, Voxel Shaders Externes & Ergonomie
> Points 1 (timeline, hors keyframes de transformation), 2 (shaders externes, format MagicaVoxel `shader/*.txt` plutôt que `// @arg`) et 3 (View Cube, glisser-déposer, auto-backup) faits. Restent dans `ROADMAP.md` : améliorations du path tracer, imports/exports, raccourcis configurables.

1. **Timeline d'Animation (`crates/mv-core` & `crates/mv-app`)** :
   - Stocker `frames: Vec<VoxelGrid>` et `active_frame: usize` par modèle.
   - Ajouter une barre de timeline permettant d'ajouter (`+`), supprimer (`-`), dupliquer et jouer les frames en boucle, ainsi que d'exécuter `xs -n <frames>` sur l'animation.
2. **Chargeur de Voxel Shaders Externes (`shaders/`)** :
   - Permettre à la commande `xs <nom>` de chercher un fichier `shaders/<nom>.wgsl` ou `shaders/<nom>.xs`, de parser les commentaires `// @arg name default min max` et d'afficher les sliders correspondants dans l'interface `egui`.
3. **Ergonomie UI** :
   - Dessiner un **View Cube 3D** interactif dans le coin supérieur droit du viewport.
   - Gérer les événements `ctx.input(|i| i.raw.dropped_files)` dans `mv-app` pour ouvrir un fichier `.vox` par simple **Drag & Drop**.
   - Ajouter une sauvegarde automatique dans `cache/backup.vox` toutes les 25 modifications.

---

## 5. Règle Obligatoire : GitNexus comme Mémoire du Projet

> [!IMPORTANT]
> **Utilisation obligatoire de GitNexus comme mémoire et intelligence de code :**
> L'utilisation de **GitNexus** (via ses outils MCP `gitnexus_*` et ses ressources `gitnexus://repo/magicavoxel/...`) comme mémoire structurelle et graphe de connaissance du projet est **strictement obligatoire** pour toute exploration, analyse d'impact, refactoring ou modification du code. Maintenez l'index à jour (`npx gitnexus analyze`) après les changements structurels.

<!-- gitnexus:start -->
# GitNexus — Code Intelligence & Mandatory Project Memory

This project is indexed by GitNexus as **magicavoxel** (1296 symbols, 4061 relationships, 112 execution flows). **Using GitNexus as the project's structural memory is MANDATORY.** Always use the GitNexus MCP tools to understand code, recall architecture/flows, assess impact, and navigate safely.

> If any GitNexus tool warns the index is stale, run `npx gitnexus analyze` in terminal first.

## Always Do

- **MUST use GitNexus as your primary project memory** before searching or modifying the codebase (`gitnexus_query`, `gitnexus_context`, and `gitnexus://repo/magicavoxel/...` resources).
- **MUST run impact analysis before editing any symbol.** Before modifying a function, class, or method, run `gitnexus_impact({target: "symbolName", direction: "upstream"})` and report the blast radius (direct callers, affected processes, risk level) to the user.
- **MUST run `gitnexus_detect_changes()` before committing** to verify your changes only affect expected symbols and execution flows.
- **MUST warn the user** if impact analysis returns HIGH or CRITICAL risk before proceeding with edits.
- When exploring unfamiliar code, use `gitnexus_query({query: "concept"})` to find execution flows instead of grepping. It returns process-grouped results ranked by relevance.
- When you need full context on a specific symbol — callers, callees, which execution flows it participates in — use `gitnexus_context({name: "symbolName"})`.

## Never Do

- NEVER edit a function, class, or method without first running `gitnexus_impact` on it.
- NEVER ignore HIGH or CRITICAL risk warnings from impact analysis.
- NEVER rename symbols with find-and-replace — use `gitnexus_rename` which understands the call graph.
- NEVER commit changes without running `gitnexus_detect_changes()` to check affected scope.

## Resources

| Resource | Use for |
|----------|---------|
| `gitnexus://repo/magicavoxel/context` | Codebase overview, check index freshness |
| `gitnexus://repo/magicavoxel/clusters` | All functional areas |
| `gitnexus://repo/magicavoxel/processes` | All execution flows |
| `gitnexus://repo/magicavoxel/process/{name}` | Step-by-step execution trace |

## CLI

| Task | Read this skill file |
|------|---------------------|
| Understand architecture / "How does X work?" | `.claude/skills/gitnexus/gitnexus-exploring/SKILL.md` |
| Blast radius / "What breaks if I change X?" | `.claude/skills/gitnexus/gitnexus-impact-analysis/SKILL.md` |
| Trace bugs / "Why is X failing?" | `.claude/skills/gitnexus/gitnexus-debugging/SKILL.md` |
| Rename / extract / split / refactor | `.claude/skills/gitnexus/gitnexus-refactoring/SKILL.md` |
| Tools, resources, schema reference | `.claude/skills/gitnexus/gitnexus-guide/SKILL.md` |
| Index, status, clean, wiki CLI commands | `.claude/skills/gitnexus/gitnexus-cli/SKILL.md` |

<!-- gitnexus:end -->
