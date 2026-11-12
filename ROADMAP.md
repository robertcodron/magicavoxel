# Roadmap — Portage de MagicaVoxel en Rust (`magicavoxel-rs`)

## État d'Avancement Global

| Phase | Module | Statut | Progression |
| :--- | :--- | :---: | :---: |
| **Phase 1** | Noyau de données (`VoxelGrid`, `Palette`, `Material`, `History`) & Format `.vox` / `.obj` / `.ply` / `slice` | ✅ Terminé | 100 % |
| **Phase 2** | Fenêtrage Desktop (`eframe`/`egui`), Caméra orbitale & Moteur Ray-Casting 3D DDA (`mv-render`) | ✅ Terminé | 100 % |
| **Phase 3** | *Model Editor* : Brushes (`V`, `F`, `B`, `L`, `C`, `P`), Actions (`T`, `R`, `G`, `N`), Symétries (`Mirror`, `Axis`) & Sélections | ✅ Terminé | 100 % |
| **Phase 4** | Moteur de Commandes Console (`docs/mv_commands.html`) & Voxel Shaders intégrés (`xs`) | ✅ Terminé | 100 % |
| **Phase 5** | *World Editor* : Graphe de scène multi-objets (`nTRN`, `nGRP`, `nSHP`), Instances, Calques (`LAYR`), Gizmos & Booléens | ✅ Terminé | 100 % |
| **Phase 6** | *Renderer* : Path Tracer GPU (`wgpu` / WGSL), Matériaux PBR (Metal, Glass, Emit, Cloud), Formes de voxels, Bloom HDR & ACES | ✅ Terminé | 100 % |
| **Phase 7** | Timeline d'Animation (`0..360`), Shaders GLSL/WGSL externes (`shaders/`), Formats QB & Marching Cubes, Hotkeys & UI avancée | ✅ Terminé | 100 % |
| **Phase 8** | **Outils Procéduraux & Ergonomie "Cozy Builder" (Inspirations *Tiny Glade*)** : Gizmos d'Extrusion, Splines & Smart Brushes, Bâtiments modulaires, Menu Radial, Lanternes & Presets d'Ambiance | ✅ Terminé | 100 % |

---

## Détail des Phases Réalisées (✅)

### ✅ Phase 1 : Fondations, Structures de Données & Format `.vox` (`mv-core` & `mv-format`)

- [x] **`VoxelGrid` (`crates/mv-core/src/voxel.rs`)** : Grille 3D jusqu'à `256x256x256` en convention MagicaVoxel (`X` = droite, `Y` = profondeur, `Z` = hauteur/up), `u8` par voxel (`0` = vide, `1..=255` = index palette).
- [x] **Palette 256 couleurs (`crates/mv-core/src/palette.rs`)** : Palette MagicaVoxel par défaut, génération `bw` / `mac`, masquage des couleurs inutilisées (`pal mask`), tri `pal sort [+-hsvrgb]`, dégradés, swap.
- [x] **Matériaux PBR (`crates/mv-core/src/material.rs`)** : `Diffuse`, `Metal`, `Glass`, `Emit`, `Blend`, `Cloud` avec paramètres `rough`, `spec`, `ior`, `att`, `flux`, `emit`, `metal`, `alpha`, `density`.
- [x] **Undo/Redo compressé (`crates/mv-core/src/history.rs`)** : Pile de 100 états avec compression RLE (`CompressedSnapshot`).
- [x] **Format `.vox` & Exports (`crates/mv-format/src/lib.rs`)** :
  - Lecture/écriture binaire `.vox` (v150) : chunks `VOX`, `MAIN`, `SIZE`, `XYZI`, `RGBA`, `MATL`.
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

### ✅ Phase 5 : Le *World Editor* (Graphe de Scène Multi-Objets, Gizmos, Patterns & Accélération BVH)

- [x] **Graphe de scène (`crates/mv-core/src/scene.rs`)** :
  - `SceneNode` = un `nTRN` + son enfant (`NodeKind::Group` / `NodeKind::Shape { model_id }`), `Transform` (`Rotation` permutation signée encodée comme l'octet `_r`, translation `IVec3`).
  - Instances partagées (*Reference*, `instantiate`) vs copies (*Duplicate*, `duplicate`, `convert_reference_to_duplicate` pour `Ctrl+T`), étendu récursivement aux hiérarchies de groupes complètes (`clone_tree`).
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
- [x] **Gizmos 3D & Manipulation Directe (`mv-app/src/world_editor.rs`)** :
  - Gizmos 3D interactifs dans le viewport avec axes projetés (X rouge, Y vert, Z bleu) et plan XY central.
  - Déplacement contraint par axe ou plan avec projection mathématique rayon-droite 3D exacte.
  - Aimantation / Snapping aux surfaces d'autres instances de la scène avec `Ctrl + LButton`.
- [x] **Bibliothèque de Patterns & Préfabriqués (`mv-core/src/voxel.rs`, `mv-app`, `mv-mcp`)** :
  - Génération de motifs procéduraux d'architecture : Arche (`sample_arch`), Colonne (`sample_column`), Escalier (`sample_stairs`), Mur crénelé (`sample_wall`), Arbre (`sample_tree`), Monument.
  - Sélecteur de motifs dans le panneau latéral droit du World Editor.
  - Parité MCP complète : outil `scene_manage (action: "spawn_pattern")`.
- [x] **Path Tracer et exports sur toute la scène** :
  - Structure à deux niveaux dans `pathtrace.wgsl` : modèles concaténés (un seul exemplaire par modèle partagé) + buffer d'instances (matrice inverse, décalage, AABB monde) ; chaque rayon teste l'AABB de chaque instance puis parcourt la grille du modèle en espace local. Repli CPU via `WorldView`.
  - Mode Render en coordonnées monde (cadrage sur l'objet édité, `F4` = scène, mise au point DOF et pipette par picking monde).
  - Exports `.obj` (un objet par shape, faces correctement orientées même pour les rotations miroir) et `.ply` sur la scène visible ; la bande PNG reste par modèle.
- [x] **Accélération Spatiale BVH (`crates/mv-render/src/world.rs`)** :
  - Hiérarchie de volumes englobants AABB (`WorldBvh`) avec partitionnement par centroïdes / SAH pour accélérer le raycasting CPU (`WorldView::raycast`) et l'occlusion des ombres (`WorldView::occluded`).
  - Passage d'une complexité linéaire `O(N)` à une traversée logarithmique `O(log N)` pour les scènes à grand nombre d'instances.

---


## Phases Complétées (✅ 100 %)


### ✅ Phase 7 : Animation, Voxel Shaders GLSL/WGSL Externes & Finitions Avancées

**Objectif :** Compléter les fonctionnalités de production (animation, shaders personnalisés, formats additionnels, raccourcis).

- [x] **Timeline d'Animation** :
  - Frames par shape (`NodeKind::Shape { models }`, un modèle par frame, boucle sur sa propre longueur) ; `Scene::current_frame` pilote rendu, picking et opérations.
  - `.vox` : `nSHP` multi-modèles (`_f`, clés éparses étendues), anciens fichiers multi-modèles = un objet animé, frames `nTRN` supplémentaires conservées.
  - Timeline (barre du bas) : frames cliquables, lecture en boucle (fps), `+` (copie), `+ vide`, suppression ; vue globale en World Editor.
  - Undo par modèle (historiques conservés en changeant de frame) ; ajout/suppression de frames annulables depuis le World Editor.
  - `xs -n N -prev|-cur <shader>` génère N frames (une étape d'undo).
- [x] **Animation & Rendu de scène** :
  - Évaluation des keyframes de transformation et rendu multi-frames.
  - Rendu Path Tracer sur la frame courante de la scène multi-objets.
- [x] **Voxel Shaders externes compatibles MagicaVoxel (`shader/*.txt`, GLSL)** :
  - Exécution GPU (`crates/mv-render/src/voxel_shader.rs`) : compute shader GLSL 450 compilé par naga (`wgpu` feature `glsl`).
  - API MagicaVoxel : `voxel()`, `color_sel()`, `palette()`, `i_volume_size`, `i_color_index`, `i_num_color_sels`, `i_iter`, `i_axis`, `i_mirror`, `i_args[16]`, `iFrame`, `iNumFrames`, `iIter`, `iRand`.
  - 51/51 shaders de `lachlanmcdonald/magicavoxel-shaders` compilent et s'exécutent.
- [x] **Améliorations du Path Tracer (`mv-render`)** :
  - Formes de voxels alternatives : `Cube`, `Lego` (téton cylindrique), `Sphere`, `Cylinder`, `Clay` (chanfrein adouci).
  - Filtre de **Bloom** HDR étiré et tone mapping ACES.
  - Éclairage ponctuel omnidirectionnel dynamique (`PointLight`) avec ombrage et Next-Event Estimation.
- [x] **Export glTF 2.0 de la scène** (`crates/mv-format/src/gltf.rs`, `.glb` / `.gltf`) :
  - Hiérarchie groupes/objets, instances partagées, greedy meshing par couleur, matériaux PBR.
- [x] **Imports / Exports Supplémentaires (`mv-format`)** :
  - Export et import **Qubicle (`.qb`)** binaire multi-matrices avec compression RLE et formats de couleur RGBA/BGRA.
  - Import et génération de terrain depuis **Heightmaps PNG** avec normalisation et étagement de palette (eau, terre, herbe, roche, neige).
  - Extraction d'iso-surface lissée **Marching Cubes** et export `.obj` et `.ply`.
- [x] **Ergonomie UI (`mv-app`)** :
  - **View Cube** 3D interactif avec orientation de la caméra et vues axiales.
  - **Glisser-déposer** d'un `.vox`, `.qb` ou heightmap `.png` dans la fenêtre.
  - **Auto-backup** toutes les 25 modifications dans `cache/backup.vox`.
  - **Confirmation des modifications non sauvegardées** avant ouverture ou fermeture.
  - **Configuration des raccourcis clavier personnalisables** via `config/hotkey.txt`.

---

### ✅ Phase 8 : Outils Procéduraux & Ergonomie "Cozy Builder" (Inspirations *Tiny Glade*)

**Objectif :** Transposer les concepts majeurs de construction organique, d'interaction directe et d'ambiance poétique de *Tiny Glade* dans l'architecture voxel de `magicavoxel-rs`, avec parité complète UI et MCP.

#### 1. Gizmos 3D de Manipulation Directe dans le Viewport (`mv-render` & `mv-app`)

- [x] **Manipulateurs 3D interactifs** :
  - Gizmo d'axes (flèches X, Y, Z + plans XY) pour déplacer les objets sélectionnés dans le *World Editor*.
  - Poignées d'extrusion directe sur les 6 faces de la boîte englobante ou de la sélection dans le *Model Editor*.
  - Aimantation intelligente aux surfaces (`Ctrl + LButton`) et incrément discret en voxels (`Snap 1/2/4/8`).
  - **Parité MCP** : outil `gizmo_transform` (actions `extrude`, `translate`, `snap_config`) et commande console `extrude [axis] [amount]`.

#### 2. Brushes Procéduraux Intelligents & Splines de Murs (`mv-core/src/spline.rs`)

- [x] **Tracé de murs par Spline 3D** :
  - Courbes de Bézier cubiques et splines Catmull-Rom 3D avec échantillonnage régulier.
  - Voxelisation procédurale en continu : génération de murs en maçonnerie de pierre avec crénelages adaptatifs et clôtures en bois.
- [x] **Intersections & Raccordements Contextuels ("Smart Snapping")** :
  - Découpe géométrique instantanée d'une arche voûtée cintrée (`carve_arch`) avec chambranle et linteau en pierre.
  - **Parité MCP** : outil `smart_brush` (actions `curve_wall`, `smart_arch`, `fence`) et commandes console `wall_spline`, `arch`.


#### 3. Bâtiments Modulaires & Détails Architecturaux Contextuels (`mv-core/src/architecture.rs`)

- [x] **Générateur procédural de toitures** :
  - Toitures automatiques : deux pans (`Gabled`), quatre pans / croupe (`Hipped`), mansarde (`Mansard`), tourelle conique (`Conical`) avec débord configurable.
- [x] **Perçage & Habillage automatique de baies (Portes, Fenêtres, Cheminées)** :
  - Baies de fenêtres avec encadrement, vitrage en retrait et volets latéraux (`punch_window`).
  - Portes avec chambranle bois/pierre et panneau battant (`punch_door`).
  - Cheminées murales en brique évidées avec mitron décoratif (`build_chimney`).
  - **Parité MCP** : outil `architecture_generate` (actions `roof`, `window`, `door`, `chimney`) et commande console `roof <type> [args]`.

#### 4. Menu Radial Contextuel In-Situ ("Pie Menu") (`mv-app/src/ui/radial_menu.rs`)

- [x] **Palette & Outils sous le curseur** :
  - Invocation rapide par clic droit ou touche `Espace` ouvrant une roue radiale circulaire centrée sur la souris.
  - Sélection circulaire : nuancier de couleurs récentes, brushes (`V/F/B/L/C/P`), outils Cozy (`Wall`, `Arch`, `Roof`, `Door`, `Lantern`, `Tree`, `Pond`).
  - Détection d'angle fluide, animation de survol et fermeture immédiate sur clic ou relâchement.

#### 5. Système de Lanternes & Éclairage Ponctuel Dynamique (`mv-render` & `mv-core/src/light.rs`)

- [x] **Props lumineux prêts à l'emploi** :
  - Placement en 1 clic de lanternes murales en fer forgé, suspendues ou sur pied (`spawn_lantern`).
- [x] **Sources de lumière ponctuelles (Point Lights)** :
  - Gestion des point lights locales (position, rayon d'action, couleur RGB, intensité).
  - Intégration dans le ray-caster temps réel (atténuation quadratique, ombrage doux) et dans le Path Tracer GPU (`pathtrace.wgsl` : évaluation analytique Next-Event Estimation).
  - **Parité MCP** : outil `light_manage` (actions `add`, `remove`, `clear`, `list`, `spawn_lantern`) et commande console `light add|set|del`.


#### 6. Biomes, Eau, Végétation & Presets d'Ambiance Dynamique (`mv-core/src/biome.rs` & `mv-render`)

- [x] **Outils de biome & végétation organique** :
  - Brosse de sentier : sentier de pavés et terre battue le long de splines (`paint_path`).
  - Brosse d'étang / eau : creusement parabolique et remplissage de surface liquide (`carve_pond`).
  - Brosse de flore : lierre procédural grimpant sur les parois verticales (`grow_ivy`).
  - Génération d'arbres stylisés "boules cotonneuses" en un clic avec tronc et branchages (`generate_fluffy_tree`).
- [x] **Presets atmosphériques & Rendu cinématique** :
  - Presets en 1 clic dans l'UI et le moteur de rendu : *Golden Hour*, *Midnight Fairytale*, *Misty Morning*, *Default*.
  - Profondeur de champ cinématique (Thin-Lens Bokeh) et filtre de Bloom HDR.
  - **Parité MCP** : actions `paint_path`, `pond`, `grow_ivy`, `tree` dans `smart_brush`.
