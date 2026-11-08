# Roadmap — Portage de MagicaVoxel en Rust (`magicavoxel-rs`)

## État d'Avancement Global

| Phase | Module | Statut | Progression |
| :--- | :--- | :---: | :---: |
| **Phase 1** | Noyau de données (`VoxelGrid`, `Palette`, `Material`, `History`) & Format `.vox` / `.obj` / `.ply` / `slice` | ✅ Terminé | 100 % |
| **Phase 2** | Fenêtrage Desktop (`eframe`/`egui`), Caméra orbitale & Moteur Ray-Casting 3D DDA (`mv-render`) | ✅ Terminé | 100 % |
| **Phase 3** | *Model Editor* : Brushes (`V`, `F`, `B`, `L`, `C`, `P`), Actions (`T`, `R`, `G`, `N`), Symétries (`Mirror`, `Axis`) & Sélections | ✅ Terminé | 100 % |
| **Phase 4** | Moteur de Commandes Console (`docs/mv_commands.html`) & Voxel Shaders intégrés (`xs`) | ✅ Terminé | 100 % |
| **Phase 6** | *Renderer* : Path Tracer GPU (`wgpu` / WGSL), Matériaux PBR (Metal, Glass, Emit, Cloud), DOF & ACES | ✅ Terminé | 90 % |
| **Phase 5** | *World Editor* : Graphe de scène multi-objets (`nTRN`, `nGRP`, `nSHP`), Instances, Calques (`LAYR`), Gizmos & Booléens | 🚧 En cours | 80 % |
| **Phase 7** | Timeline d'Animation (`0..360`), Shaders GLSL/WGSL externes (`shaders/`), OIDN Denoiser, Marching Cubes & UI avancée | ⏳ À faire | 0 % |

---

## Détail des Phases Réalisées (✅)

### ✅ Phase 1 : Fondations, Structures de Données & Format `.vox` (`mv-core` & `mv-format`)
- [x] **`VoxelGrid` (`crates/mv-core/src/voxel.rs`)** : Grille 3D jusqu'à `256x256x256` en convention MagicaVoxel (`X` = droite, `Y` = profondeur, `Z` = hauteur/up), `u8` par voxel (`0` = vide, `1..=255` = index palette).
- [x] **Palette 256 couleurs (`crates/mv-core/src/palette.rs`)** : Palette MagicaVoxel par défaut, génération `bw` / `mac`, masquage des couleurs inutilisées (`pal mask`), tri `pal sort [+-hsvrgb]`, dégradés, swap.
- [x] **Matériaux PBR (`crates/mv-core/src/material.rs`)** : `Diffuse`, `Metal`, `Glass`, `Emit`, `Blend`, `Cloud` avec paramètres `rough`, `spec`, `ior`, `att`, `flux`, `emit`, `metal`, `alpha`, `density`.
- [x] **Undo/Redo compressé (`crates/mv-core/src/history.rs`)** : Pile de 100 états avec compression RLE (`CompressedSnapshot`).
- [x] **Format `.vox` & Exports (`crates/mv-format/src/lib.rs`)** :
  - Lecture/écriture binaire `.vox` (v150) : chunks `VOX `, `MAIN`, `SIZE`, `XYZI`, `RGBA`, `MATL`.
  - Export Wavefront `.obj` avec **Greedy Meshing 2D par tranche** (`export_obj_greedy`).
  - Export nuage de points Stanford `.ply` (`export_ply_point_cloud`).
  - Export bande de tranches 2D `.png` (`export_slice_png`).

### ✅ Phase 2 : Fenêtrage, UI Desktop & Ray-Caster 3D DDA (`mv-render` & `mv-app`)
- [x] **Ray-Caster 3D DDA (`crates/mv-render/src/lib.rs`)** : Algorithme d'Amanatides & Woo parallélisé via `rayon`.
- [x] **Aides visuelles Éditeur** : Ambient Occlusion analytique par sommet de voxel, arêtes (`Ctrl+E`), grille au sol (`Ctrl+G`), boîte englobante (`Ctrl+F`), sol (`Ctrl+U`), fond (`Ctrl+B`), ombres portées directionnelles (`Ctrl+W`), wireframe (`Ctrl+M`).
- [x] **Ray-Picking 3D exact (`pick_voxel`)** : Retourne les coordonnées exactes du voxel touché, la normale de la face et la distance `t` (avec support du plan `z = 0` pour poser des voxels sur un canevas vide).
- [x] **Caméra (`OrbitCamera`)** : Rotation (`RButton`), Pan (`MButton` ou `Space+LButton`), Zoom (`Wheel`), `Recenter (F4/4)`, `Snap 90° (F5/5)`, `X+LButton` (focus center), et 10 slots caméra (`F7`/`F8`).

### ✅ Phase 3 : *Model Editor* — Brushes, Sélections & Symétries (`mv-core/src/brush.rs`)
- [x] **Actions de Brush** : `Attach (T)`, `Erase (R)`, `Paint (G)`, `Select (N)` (avec inversion temporaire `Shift` et soustraction de sélection `Shift+Alt`).
- [x] **Formes de Brush** : `Voxel (V)` (Cube / Sphère de taille `1..9`), `Face (F)` (flood-fill planaire contigu par couleur), `Box (B)`, `Line (L)` (Bresenham 3D), `Center (C)` (Cercle / Carré), `Pattern (P)`.
- [x] **Symétries temps réel** : `Mirror Mode (1/2/3)` sur X, Y, Z et `Axis Mode (Ctrl+1/2/3)` sur X, Y, Z avec prévisualisation 3D directe dans le viewport.
- [x] **Sélections (`crates/mv-core/src/selection.rs`)** : `SelectionMask` 3D + presse-papiers `VoxelClipboard` (`Ctrl+A`, `Ctrl+I`, `Ctrl+D`, `Ctrl+C`, `Ctrl+X`, `Ctrl+V`, `Backspace/Delete`).

### ✅ Phase 4 : Moteur de Commandes Console & Voxel Shaders (`mv-core/src/command.rs`)
- [x] **Toutes les commandes de `docs/mv_commands.html`** : `sel_all`, `sel_none`, `sel_inv`, `copy`, `cut`, `paste`, `size`, `shrink`, `zero`, `full`, `fill`, `inv`, `flip`, `loop`, `scale`, `x2`, `rot90`, `rot`, `repeat`, `mir`, `dia`, `dil`, `ero`, `flood`, `noise`, `rand`, `maze`, `shear`, `pal [mask|fill|bw|mac|sort]`, `log`, `o`, `odir`.
- [x] **Voxel Shaders procéduraux (`xs`)** : `xs [-n iter] <wave|sine|sphere|torus|cylinder|gyroid|pyramid|checker|life> [args...]`.

### ✅ Phase 6 : *Path Tracer* GPU (`wgpu`) & Post-Processing (`mv-render/src/gpu_pathtracer.rs`)
- [x] **Compute Shader WGSL (`crates/mv-render/src/pathtrace.wgsl`)** :
  - Accumulation progressive Monte-Carlo multi-rebonds sur GPU (`wgpu`) avec fallback CPU automatique.
  - Matériaux PBR : `Diffuse` (Next-Event Estimation + rebond cosinus), `Metal` (GGX microfacet), `Glass` (réfraction IOR + Fresnel Schlick + absorption Beer-Lambert + ombres translucides), `Emit` (illumination globale HDR), `Cloud` (diffusion volumétrique).
  - Caméra **Thin-Lens Depth of Field (DOF)** : ouverture `aperture` et mise au point `focus_dist` cliquable directement sur un voxel avec `LButton`.
  - Atmosphère & Post-processing : Soleil directionnel, ciel Rayleigh, brouillard exponentiel, exposition, vignette, **ACES Filmic Tone Mapping** et export PNG (`F6`).

---

## Prochaines Phases à Implémenter (⏳)

### ⏳ Phase 5 : Le *World Editor* (Graphe de Scène Multi-Objets & Calques)
**Objectif :** Supporter des scènes multi-objets hiérarchiques jusqu'à `2048x2048x1024` comme spécifié dans [`docs/mv_main.html`](file:///Users/robbie/Code/magicavoxel/docs/mv_main.html#L584-L603) et [`docs/mv_controls.html`](file:///Users/robbie/Code/magicavoxel/docs/mv_controls.html#L188-L254).
- [x] **Graphe de scène (`crates/mv-core/src/scene.rs`)** :
  - `SceneNode` = un `nTRN` + son enfant (`NodeKind::Group` / `NodeKind::Shape { model_id }`), `Transform` (`Rotation` permutation signée encodée comme l'octet `_r`, translation `IVec3`).
  - Instances partagées (*Reference*, `instantiate`) vs copies (*Duplicate*, `duplicate`, `convert_reference_to_duplicate` pour `Ctrl+T`).
  - 8 calques par défaut (`SceneLayer`), visibilité héritée (nœud, ancêtres, calque).
  - Opérations : `world_bounds`, `translate_world`, `move_to_ground` (`G`), `rotate90` (`-`/`+`, axe monde), `union` (`U`), `remove_node`, `compact_models`, `merge_to_grid`.
  - `EditorProject` : `scene` + `active_model` ; `grid` est le modèle en cours d'édition (`set_active_model`, `commit_active_model`, `model()`).
- [x] **Sérialisation `.vox` multi-modèles (`crates/mv-format/src/scene_io.rs`)** :
  - Lecture/écriture de toutes les paires `SIZE` + `XYZI`, `nTRN`, `nGRP`, `nSHP`, `LAYR` (résolution par id, ordre des chunks indifférent, protection contre les cycles).
  - Fichiers pré-0.99 multi-modèles (animations) : frame 0 visible, autres frames en shapes cachées `frame N`.
  - Validé sur 15 fichiers réels (`ephtracy/voxel-model`, `opengametools`, `dot_vox`) via `cargo run -p mv-format --example vox_info -- <fichiers.vox>`.
  - Limites actuelles : seule la première frame d'animation des `nTRN`/`nSHP` est conservée ; `rOBJ`, `rCAM`, `NOTE`, `IMAP` sont ignorés.
- [x] **World Editor dans l'app (`crates/mv-app/src/world_editor.rs`, `ui/world_panel.rs`)** :
  - Mode `🌍 World` ; `TAB` bascule Model ↔ World (ouvre l'objet sélectionné / entre dans le groupe sélectionné), `Shift+TAB` quitte le groupe, `F1` ouvre la console.
  - Viewport : sélection au clic (`Shift` = ajout), double-clic pour ouvrir, glisser pour déplacer sur un plan horizontal, `Alt+clic` pipette, `X+clic` centre la caméra, `F4` cadre la sélection.
  - Raccourcis : flèches / `PgUp` / `PgDn` (relatifs à la caméra, `Shift` ×10), `G` sol, `-`/`+` rotation (Z, `Shift` X, `Alt` Y), `U` union, `Ctrl+N` nouvel objet, `Suppr`, `Ctrl+T`, `Ctrl+R` / `Ctrl+Shift+R` grouper / dégrouper, `Ctrl+H` / `Ctrl+Shift+H`, `Ctrl+A` / `Ctrl+D`.
  - Panneaux **Scene Outline** (arbre, visibilité, fil d'Ariane des groupes), **propriétés** (nom, position, calque, instance / duplicate) et **Layers** (visibilité, couleur, nom, assignation).
  - Undo/redo dédié (structure de scène uniquement, une étape par geste).
  - Rendu monde CPU (`crates/mv-render/src/world.rs`) : chaque shape est tracée dans son espace modèle, instances partagées, ombres entre objets, objets hors du groupe courant estompés.
  - Tests d'intégration headless (`crates/mv-app/src/tests.rs`) pilotant de vraies frames egui (clavier, clics, glisser).
- [ ] **Reste à faire (World Editor)** :
  - Gizmos 3D de translation/rotation à la souris, `Ctrl+LButton` (déplacer sur une surface), Pattern Library.
  - Copier/instancier un groupe entier (seules les shapes sont copiables aujourd'hui).
  - Path Tracer sur la scène entière (il rend actuellement le modèle actif seul).
- [ ] **Accélération Spatiale** :
  - Rendu monde actuel : test AABB linéaire par shape (~36 ms/frame pour 104 objets en `render_scale` 2). Ajouter une BVH CPU puis GPU.
  - Traversée BVH / Sparse Voxel Octree (ou grille de briques à 2 niveaux) dans `mv-render` pour tracer des dizaines d'instances sans les fusionner dans une grille dense unique.

### ⏳ Phase 7 : Animation, Voxel Shaders GLSL/WGSL Externes & Finitions Avancées
**Objectif :** Compléter les fonctionnalités de production (animation, shaders personnalisés, débruitage IA, imports/exports additionnels).
- [ ] **Timeline d'Animation (`0..360` frames)** :
  - Stockage de plusieurs frames par modèle et keyframes de transformation dans le graphe de scène (chunk `META`).
  - UI Timeline en haut/bas : boutons `+` (ajouter frame), `-` (supprimer), lecture auto, export de séquence d'images et rendu *Turntable* avec *Motion Blur*.
- [ ] **Moteur de Voxel Shaders Externes (`shaders/*.xs` / `.wgsl`)** :
  - Chargement dynamique de fichiers shaders depuis un dossier `shaders/`.
  - Parsing des annotations `// @arg name default min max` pour générer automatiquement des sliders dans l'interface `egui`.
- [ ] **Améliorations du Path Tracer (`mv-render`)** :
  - Éclairage **IBL (`.hdr` equirectangular)** avec *Importance Sampling*.
  - Formes de voxels alternatives dans le ray-tracer : *Lego*, *Sphere*, *Cylinder*, *Clay*, *Marching Cubes*.
  - Filtre de **Bloom** HDR étiré et **Intel Open Image Denoise (OIDN)** (avec buffers auxiliaires Albedo + Normal).
- [ ] **Imports / Exports Supplémentaires (`mv-format`)** :
  - Export **Marching Cubes** (`.ply` / `.obj`) et **glTF 2.0 (`.gltf` / `.glb`)** avec atlas de textures UV et **Light Baking** (AO par sommet/face).
  - Import de formats `.qb`, `.xraw`, `.schematic`, heightmaps `.png` et voxelisation de maillages `.obj`.
- [ ] **Ergonomie UI (`mv-app`)** :
  - **View Cube 3D** interactif dessiné dans le coin supérieur droit du viewport.
  - **Drag & Drop** de fichiers `.vox` / `.png` directement dans la fenêtre.
  - **Auto-Backup** asynchrone toutes les 25 actions dans `cache/backup.vox` et personnalisation des raccourcis via `config/hotkey.txt`.

