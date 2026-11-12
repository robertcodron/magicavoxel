# MagicaVoxel MCP Server (`mv-mcp`)

Serveur **Model Context Protocol (MCP)** officiel pour **MagicaVoxel Rust Edition** (`magicavoxel-rs`).
Il fonctionne sur `stdio` (JSON-RPC 2.0, un message par ligne) et permet aux assistants IA de créer, éditer, inspecter et exporter des modèles et des scènes voxel complètes.

---

## 1. Démarrage Rapide

### Compilation
```bash
cargo build --release -p mv-mcp
```
Le binaire est produit dans `target/release/mv-mcp`.

### Configuration client

#### Claude Code
```bash
claude mcp add magicavoxel -- /chemin/vers/target/release/mv-mcp
```

#### Client MCP générique (`claude_desktop_config.json` ou équivalent)
```json
{
  "mcpServers": {
    "magicavoxel": {
      "command": "/chemin/vers/target/release/mv-mcp"
    }
  }
}
```

---

## 2. Conventions Techniques

- **Axes Z-up** :
  - `X` : droite (`0..size_x - 1`)
  - `Y` : profondeur (`0..size_y - 1`)
  - `Z` : hauteur (`0..size_z - 1`, le sol est `z = 0`)
- **Index de Palette** :
  - `0` : vide / transparent (efface un voxel)
  - `1..=255` : couleurs de palette
- **Atomicité & Annulation** :
  - Chaque appel d'outil réussi crée **une seule étape d'annulation** (`history`).
  - Si un argument est invalide ou hors limites, l'outil échoue (`isError: true`) sans modifier l'état.

---

## 3. Liste Complète des Outils (27 Outils)

### Gestion de Projet & Formats (7 outils)

| Outil | Rôle & Capacités |
| :--- | :--- |
| `project_new` | Crée un nouveau projet vide avec dimensions `size_x`, `size_y`, `size_z` (1..=256) et nom optionnel. |
| `project_open` | Charge un fichier de projet : MagicaVoxel `.vox`, Qubicle `.qb` ou heightmap de terrain `.png` (`max_height`). |
| `project_save` | Enregistre le projet courant en format `.vox`. |
| `project_export` | Exporte la scène ou le modèle dans divers formats 3D et 2D : `vox`, `glb`, `gltf`, `obj`, `ply`, `slice_png`, `qb`, `heightmap_png`, `marching_cubes_obj`, `marching_cubes_ply`. |
| `project_info` | Retourne l'état complet du projet au format JSON (dimensions, voxels solides, nœuds de scène, calques, animation). |
| `console_command` | Exécute une commande console MagicaVoxel (`size`, `mir`, `rot90`, `xs`, `flood`, `noise`...). |
| `history` | Annule (`undo`) ou refait (`redo`) une action dans le modèle (`scope: model`) ou dans la scène (`scope: world`). |

### Dessin de Voxels & Shaders (9 outils)

| Outil | Rôle & Capacités |
| :--- | :--- |
| `voxel_set` | Pose ou efface des voxels individuels `[x, y, z]` ou `[x, y, z, color]`. |
| `voxel_box` | Dessine une boîte pleine ou creuse (`min`, `max`, `color`, `hollow`, `wall_thickness`). |
| `voxel_sphere` | Dessine une sphère ou un ellipsoïde (`center`, `radius` ou `radii`, `color`, `hollow`). |
| `voxel_cylinder` | Dessine un cylindre, cône ou tronc de cône (`center`, `axis`, `height`, `radius`, `radius_top`). |
| `voxel_line` | Trace un segment 3D entre `start` et `end`, avec épaisseur optionnelle `radius`. |
| `voxel_batch` | Exécute une suite d'opérations géométriques de manière atomique (recommandé pour sculpter des objets). |
| `voxel_get` | Lit les voxels non vides d'une région englobante (JSON, plafonné à 5000 voxels). |
| `voxel_clear` | Vide intégralement le modèle édité. |
| `voxel_shader` | Liste (`list`) ou exécute (`run`) des Voxel Shaders procéduraux GPU (`xs`). |

### Palette & Matériaux PBR (2 outils)

| Outil | Rôle & Capacités |
| :--- | :--- |
| `palette_manage` | Lit (`get`), sélectionne (`select`), ou modifie (`set`) les 256 couleurs RGBA de la palette. |
| `material_manage` | Règle les propriétés PBR associées aux 256 index de palette (`diffuse`, `metal`, `glass`, `emit`, `cloud`, `blend`). |

### Éditeur de Monde & Animation (3 outils)

| Outil | Rôle & Capacités |
| :--- | :--- |
| `scene_manage` | Graphe de scène multi-objets : `create_object`, `edit_object`, `translate`, `move_to`, `rotate90`, `move_to_ground`, `instantiate`, `duplicate`, `union`, `group`, `ungroup`, `hide`, `show`, `spawn_pattern`. |
| `layer_manage` | Gère les 8 calques de visibilité de la scène (nom, couleur, masquage). |
| `animation` | Navigue et modifie les frames d'animation du modèle édité (`info`, `set`, `add`, `duplicate`, `delete`). |

### Caméra & Rendu (2 outils)

| Outil | Rôle & Capacités |
| :--- | :--- |
| `render_preview` | Génère un rendu PNG base64 en mode `model`, `world` ou `pathtracer` (Path Tracer GPU progressif). |
| `camera` | Lit (`get`) ou règle (`set`) la position, cible, angle orbital, fov et distance de la caméra. |

### Outils Cozy & Procéduraux (4 outils)

| Outil | Actions & Capacités |
| :--- | :--- |
| `smart_brush` | **Outils de tracé et biomes organiques :**<br>• `curve_wall` : mur spline 3D avec épaisseur et créneaux.<br>• `smart_arch` : arcade voûtée avec chambranle en pierre.<br>• `fence` : clôture en bois avec poteaux et traverses.<br>• `paint_path` : chemin pavé et terre le long d'une spline.<br>• `grow_ivy` : feuillage de lierre grimpant sur les murs.<br>• `pond` : bassin d'eau incurvé avec excavation d'air.<br>• `tree` : arbre stylisé avec tronc en bois et feuillage multilobé. |
| `architecture_generate` | **Générateur architectural procédural :**<br>• `roof` : toiture gabled, hipped, mansard ou conique avec débord.<br>• `window` : baie vitrée avec encadrement, vitrage en retrait et volets.<br>• `door` : ouverture de porte avec chambranle et vantail en bois.<br>• `chimney` : conduit de cheminée en brique creux avec mitron sommital. |
| `light_manage` | **Éclairage & accessoires d'ambiance :**<br>• `add` : ajoute une lumière ponctuelle (`pos`, `radius`, `color`, `intensity`).<br>• `remove` : supprime une lumière ponctuelle par index.<br>• `clear` : efface toutes les lumières ponctuelles.<br>• `list` : liste les lumières actives avec leurs caractéristiques.<br>• `spawn_lantern` : place une lanterne stylisée sur un mur, plafond ou sol. |
| `gizmo_transform` | **Transformations interactives & grille :**<br>• `extrude` : extrusion de faces extérieures ou de sélection selon un axe (`x`/`y`/`z`).<br>• `translate` : translation avec magnétisme de grille pour sélection ou objets de scène.<br>• `snap_config` : configuration du pas de magnétisme (`step`, `enabled`). |
