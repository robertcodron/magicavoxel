//! World Editor scene graph: several models placed in a hierarchy of groups and shapes.
//!
//! Mirrors the `.vox` scene graph (`nTRN` / `nGRP` / `nSHP` / `LAYR`) in a simpler form:
//! each `nTRN` + child pair of the file becomes one [`SceneNode`] that carries both the
//! transform and the payload (a group of children, or a shape referencing a model).
//!
//! Several shapes may reference the same `model_id` (MagicaVoxel *Reference* / instance);
//! [`Scene::convert_reference_to_duplicate`] gives a shape its own copy (`Ctrl+T`).
//!
//! World placement of a voxel `v` of a shape (MagicaVoxel convention, models are centered):
//! `world = rotation * (v - size / 2) + translation`, integer division, composed with the
//! transforms of all ancestor groups.

use std::collections::BTreeMap;

use glam::{IVec3, Vec3};

use crate::voxel::{Axis, VoxelGrid, MAX_GRID_DIM};

pub type NodeId = usize;

/// Number of layers created by default (MagicaVoxel shows 8 layers).
pub const DEFAULT_LAYER_COUNT: usize = 8;

/// Axis-aligned rotation (signed permutation matrix), as stored in the `_r` byte of `nTRN`.
///
/// Row `i` of the matrix has a single non-zero entry at column `idx[i]`, equal to `-1`
/// when `neg[i]` is set, `+1` otherwise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rotation {
    idx: [u8; 3],
    neg: [bool; 3],
}

impl Default for Rotation {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Rotation {
    pub const IDENTITY: Self = Self {
        idx: [0, 1, 2],
        neg: [false; 3],
    };

    /// Decode the `.vox` byte: bits 0-1 = column of row 0, bits 2-3 = column of row 1,
    /// bits 4/5/6 = sign of rows 0/1/2. Invalid bytes decode to identity.
    pub fn from_vox_byte(b: u8) -> Self {
        let i0 = b & 3;
        let i1 = (b >> 2) & 3;
        if i0 > 2 || i1 > 2 || i0 == i1 {
            return Self::IDENTITY;
        }
        Self {
            idx: [i0, i1, 3 - i0 - i1],
            neg: [b & 0x10 != 0, b & 0x20 != 0, b & 0x40 != 0],
        }
    }

    pub fn to_vox_byte(self) -> u8 {
        self.idx[0]
            | (self.idx[1] << 2)
            | ((self.neg[0] as u8) << 4)
            | ((self.neg[1] as u8) << 5)
            | ((self.neg[2] as u8) << 6)
    }

    /// +90° rotation around `axis` (right-handed, Z-up).
    pub fn rot90(axis: Axis) -> Self {
        match axis {
            // (x, y, z) -> (x, -z, y)
            Axis::X => Self {
                idx: [0, 2, 1],
                neg: [false, true, false],
            },
            // (x, y, z) -> (z, y, -x)
            Axis::Y => Self {
                idx: [2, 1, 0],
                neg: [false, false, true],
            },
            // (x, y, z) -> (-y, x, z)
            Axis::Z => Self {
                idx: [1, 0, 2],
                neg: [true, false, false],
            },
        }
    }

    /// Same signed permutation applied to a float vector (exact, no rounding).
    pub fn apply_f32(self, v: Vec3) -> Vec3 {
        let a = v.to_array();
        let c = |i: usize| {
            let x = a[self.idx[i] as usize];
            if self.neg[i] {
                -x
            } else {
                x
            }
        };
        Vec3::new(c(0), c(1), c(2))
    }

    pub fn apply(self, v: IVec3) -> IVec3 {
        let a = v.to_array();
        let c = |i: usize| {
            let x = a[self.idx[i] as usize];
            if self.neg[i] {
                -x
            } else {
                x
            }
        };
        IVec3::new(c(0), c(1), c(2))
    }

    /// Matrix product `self * inner` (apply `inner` first, then `self`).
    pub fn then(self, inner: Self) -> Self {
        let mut out = Self::IDENTITY;
        for i in 0..3 {
            let j = self.idx[i] as usize;
            out.idx[i] = inner.idx[j];
            out.neg[i] = self.neg[i] ^ inner.neg[j];
        }
        out
    }

    pub fn inverse(self) -> Self {
        let mut out = Self::IDENTITY;
        for i in 0..3 {
            let j = self.idx[i] as usize;
            out.idx[j] = i as u8;
            out.neg[j] = self.neg[i];
        }
        out
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Transform {
    pub rotation: Rotation,
    pub translation: IVec3,
}

impl Transform {
    pub const IDENTITY: Self = Self {
        rotation: Rotation::IDENTITY,
        translation: IVec3::ZERO,
    };

    pub fn from_translation(t: IVec3) -> Self {
        Self {
            rotation: Rotation::IDENTITY,
            translation: t,
        }
    }

    /// `self ∘ child`: express a child-space transform in this transform's parent space.
    pub fn compose(self, child: Self) -> Self {
        Self {
            rotation: self.rotation.then(child.rotation),
            translation: self.rotation.apply(child.translation) + self.translation,
        }
    }

    pub fn apply(self, p: IVec3) -> IVec3 {
        self.rotation.apply(p) + self.translation
    }

    pub fn inverse(self) -> Self {
        let inv = self.rotation.inverse();
        Self {
            rotation: inv,
            translation: -inv.apply(self.translation),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneLayer {
    pub name: String,
    pub color: [u8; 3],
    pub hidden: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeKind {
    Group {
        children: Vec<NodeId>,
    },
    /// `models[f]` is the model shown at animation frame `f` (never empty). A shape
    /// with a single model is static; animated shapes loop over their own length.
    Shape {
        models: Vec<usize>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneNode {
    pub name: Option<String>,
    /// Transform relative to the parent group.
    pub transform: Transform,
    pub layer_id: usize,
    pub hidden: bool,
    pub parent: Option<NodeId>,
    pub kind: NodeKind,
    /// Extra `nTRN` keyframes `(frame, transform)` read from a file, written back
    /// unchanged. Transform animation is not evaluated yet: `transform` is used for
    /// every frame.
    pub extra_transform_keys: Vec<(u32, Transform)>,
}

impl SceneNode {
    /// Model of frame 0 (see [`Scene::shape_model`] for the current frame).
    pub fn model_id(&self) -> Option<usize> {
        self.frame_models().first().copied()
    }

    /// Models of every animation frame (empty for groups).
    pub fn frame_models(&self) -> &[usize] {
        match &self.kind {
            NodeKind::Shape { models } => models,
            NodeKind::Group { .. } => &[],
        }
    }

    /// Model shown at a global animation frame (shapes loop over their own length).
    pub fn model_at_frame(&self, frame: u32) -> Option<usize> {
        let models = self.frame_models();
        (!models.is_empty()).then(|| models[frame as usize % models.len()])
    }

    pub fn children(&self) -> &[NodeId] {
        match &self.kind {
            NodeKind::Group { children } => children,
            NodeKind::Shape { .. } => &[],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SceneError {
    UnknownNode(NodeId),
    NotAGroup(NodeId),
    NotAShape(NodeId),
    UnknownModel(usize),
    CannotRemoveRoot,
    NothingToMerge,
    /// The merged object would exceed the 256³ model limit.
    TooLarge(IVec3),
}

impl std::fmt::Display for SceneError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownNode(id) => write!(f, "unknown scene node {id}"),
            Self::NotAGroup(id) => write!(f, "node {id} is not a group"),
            Self::NotAShape(id) => write!(f, "node {id} is not a shape"),
            Self::UnknownModel(id) => write!(f, "unknown model {id}"),
            Self::CannotRemoveRoot => write!(f, "the root group cannot be removed"),
            Self::NothingToMerge => write!(f, "no visible voxels to merge"),
            Self::TooLarge(s) => write!(
                f,
                "merged object {}x{}x{} exceeds {MAX_GRID_DIM}^3",
                s.x, s.y, s.z
            ),
        }
    }
}

impl std::error::Error for SceneError {}

/// Snapshot of a scene's nodes and layers (see [`Scene::structure`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneStructure {
    layers: Vec<SceneLayer>,
    nodes: BTreeMap<NodeId, SceneNode>,
    root: NodeId,
    next_id: NodeId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scene {
    pub models: Vec<VoxelGrid>,
    pub layers: Vec<SceneLayer>,
    nodes: BTreeMap<NodeId, SceneNode>,
    root: NodeId,
    next_id: NodeId,
    /// Animation frame displayed and used by every geometric query. Not part of the
    /// undoable [`SceneStructure`].
    pub current_frame: u32,
}

impl Default for Scene {
    fn default() -> Self {
        Self::new()
    }
}

impl Scene {
    /// Empty scene: a root group, default layers, no models.
    pub fn new() -> Self {
        let mut nodes = BTreeMap::new();
        nodes.insert(
            0,
            SceneNode {
                name: None,
                transform: Transform::IDENTITY,
                layer_id: 0,
                hidden: false,
                parent: None,
                kind: NodeKind::Group {
                    children: Vec::new(),
                },
                extra_transform_keys: Vec::new(),
            },
        );
        Self {
            models: Vec::new(),
            layers: default_layers(),
            nodes,
            root: 0,
            next_id: 1,
            current_frame: 0,
        }
    }

    /// Scene with a single model at the origin (what a plain single-model `.vox` means).
    pub fn single(grid: VoxelGrid) -> Self {
        let mut scene = Self::new();
        let model_id = scene.add_model(grid);
        let root = scene.root;
        scene
            .add_shape(root, model_id, Transform::IDENTITY)
            .expect("root is a group");
        scene
    }

    pub fn root(&self) -> NodeId {
        self.root
    }

    pub fn node(&self, id: NodeId) -> Option<&SceneNode> {
        self.nodes.get(&id)
    }

    pub fn node_mut(&mut self, id: NodeId) -> Option<&mut SceneNode> {
        self.nodes.get_mut(&id)
    }

    /// All nodes in id order.
    pub fn nodes(&self) -> impl Iterator<Item = (NodeId, &SceneNode)> {
        self.nodes.iter().map(|(&id, n)| (id, n))
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    fn get(&self, id: NodeId) -> Result<&SceneNode, SceneError> {
        self.nodes.get(&id).ok_or(SceneError::UnknownNode(id))
    }

    fn get_mut(&mut self, id: NodeId) -> Result<&mut SceneNode, SceneError> {
        self.nodes.get_mut(&id).ok_or(SceneError::UnknownNode(id))
    }

    pub fn add_model(&mut self, grid: VoxelGrid) -> usize {
        self.models.push(grid);
        self.models.len() - 1
    }

    fn insert_child(&mut self, parent: NodeId, node: SceneNode) -> Result<NodeId, SceneError> {
        match &self.get(parent)?.kind {
            NodeKind::Group { .. } => {}
            NodeKind::Shape { .. } => return Err(SceneError::NotAGroup(parent)),
        }
        let id = self.next_id;
        self.next_id += 1;
        self.nodes.insert(
            id,
            SceneNode {
                parent: Some(parent),
                ..node
            },
        );
        if let NodeKind::Group { children } = &mut self.get_mut(parent)?.kind {
            children.push(id);
        }
        Ok(id)
    }

    pub fn add_group(
        &mut self,
        parent: NodeId,
        transform: Transform,
    ) -> Result<NodeId, SceneError> {
        self.insert_child(
            parent,
            SceneNode {
                name: None,
                transform,
                layer_id: 0,
                hidden: false,
                parent: None,
                kind: NodeKind::Group {
                    children: Vec::new(),
                },
                extra_transform_keys: Vec::new(),
            },
        )
    }

    pub fn add_shape(
        &mut self,
        parent: NodeId,
        model_id: usize,
        transform: Transform,
    ) -> Result<NodeId, SceneError> {
        if model_id >= self.models.len() {
            return Err(SceneError::UnknownModel(model_id));
        }
        self.insert_child(
            parent,
            SceneNode {
                name: None,
                transform,
                layer_id: 0,
                hidden: false,
                parent: None,
                kind: NodeKind::Shape {
                    models: vec![model_id],
                },
                extra_transform_keys: Vec::new(),
            },
        )
    }

    /// Remove a node and its whole subtree. Models are kept (see [`Scene::compact_models`]).
    pub fn remove_node(&mut self, id: NodeId) -> Result<(), SceneError> {
        if id == self.root {
            return Err(SceneError::CannotRemoveRoot);
        }
        let parent = self.get(id)?.parent;
        for n in self.subtree(id) {
            self.nodes.remove(&n);
        }
        if let Some(p) = parent {
            if let Some(NodeKind::Group { children }) = self.nodes.get_mut(&p).map(|n| &mut n.kind)
            {
                children.retain(|&c| c != id);
            }
        }
        Ok(())
    }

    /// `id` and all its descendants, depth-first, parents before children.
    pub fn subtree(&self, id: NodeId) -> Vec<NodeId> {
        let mut out = Vec::new();
        let mut stack = vec![id];
        while let Some(n) = stack.pop() {
            if let Some(node) = self.nodes.get(&n) {
                out.push(n);
                stack.extend(node.children().iter().rev().copied());
            }
        }
        out
    }

    /// Shape nodes in the subtree of `id` (including `id` itself if it is a shape).
    pub fn shapes_under(&self, id: NodeId) -> Vec<NodeId> {
        self.subtree(id)
            .into_iter()
            .filter(|n| matches!(self.nodes[n].kind, NodeKind::Shape { .. }))
            .collect()
    }

    /// Transform from the node's local space to world space.
    pub fn world_transform(&self, id: NodeId) -> Transform {
        let mut chain = Vec::new();
        let mut cur = Some(id);
        while let Some(n) = cur {
            match self.nodes.get(&n) {
                Some(node) => {
                    chain.push(node.transform);
                    cur = node.parent;
                }
                None => break,
            }
        }
        chain
            .into_iter()
            .rev()
            .fold(Transform::IDENTITY, Transform::compose)
    }

    /// Hidden itself, through an ancestor, or through its layer.
    pub fn is_visible(&self, id: NodeId) -> bool {
        let mut cur = Some(id);
        while let Some(n) = cur {
            let Some(node) = self.nodes.get(&n) else {
                return false;
            };
            let layer_hidden = self.layers.get(node.layer_id).is_some_and(|l| l.hidden);
            if node.hidden || layer_hidden {
                return false;
            }
            cur = node.parent;
        }
        true
    }

    /// Model a shape shows at the current animation frame.
    pub fn shape_model(&self, shape: NodeId) -> Option<usize> {
        self.nodes.get(&shape)?.model_at_frame(self.current_frame)
    }

    fn model_of(&self, shape: NodeId) -> Result<&VoxelGrid, SceneError> {
        self.get(shape)?;
        let model_id = self
            .shape_model(shape)
            .ok_or(SceneError::NotAShape(shape))?;
        self.models
            .get(model_id)
            .ok_or(SceneError::UnknownModel(model_id))
    }

    /// Local→world placement of model voxel coordinates for a shape.
    /// Maps a shape's model voxel coordinates to world voxel coordinates.
    pub fn shape_voxel_transform(&self, shape: NodeId) -> Result<Transform, SceneError> {
        let model = self.model_of(shape)?;
        let half = (model.dimensions() / 2).as_ivec3();
        let centering = Transform::from_translation(-half);
        Ok(self.world_transform(shape).compose(centering))
    }

    /// Inclusive world-space voxel bounds of everything under `id`, `None` if no shape.
    pub fn world_bounds(&self, id: NodeId) -> Option<(IVec3, IVec3)> {
        let mut acc: Option<(IVec3, IVec3)> = None;
        for shape in self.shapes_under(id) {
            let (Ok(model), Ok(xf)) = (self.model_of(shape), self.shape_voxel_transform(shape))
            else {
                continue;
            };
            let a = xf.apply(IVec3::ZERO);
            let b = xf.apply(model.dimensions().as_ivec3() - IVec3::ONE);
            let (lo, hi) = (a.min(b), a.max(b));
            acc = Some(match acc {
                Some((l, h)) => (l.min(lo), h.max(hi)),
                None => (lo, hi),
            });
        }
        acc
    }

    /// Move a node by a world-space offset.
    pub fn translate_world(&mut self, id: NodeId, delta: IVec3) -> Result<(), SceneError> {
        let parent_rot = match self.get(id)?.parent {
            Some(p) => self.world_transform(p).rotation,
            None => Rotation::IDENTITY,
        };
        let local = parent_rot.inverse().apply(delta);
        self.get_mut(id)?.transform.translation += local;
        Ok(())
    }

    /// `G`: move the node so that its lowest voxel rests on the ground plane `z = 0`.
    pub fn move_to_ground(&mut self, id: NodeId) -> Result<(), SceneError> {
        self.get(id)?;
        if let Some((lo, _)) = self.world_bounds(id) {
            self.translate_world(id, IVec3::new(0, 0, -lo.z))?;
        }
        Ok(())
    }

    /// `-` / `+`: rotate a node by 90° around a world axis through its own pivot.
    pub fn rotate90(&mut self, id: NodeId, axis: Axis) -> Result<(), SceneError> {
        let parent_rot = match self.get(id)?.parent {
            Some(p) => self.world_transform(p).rotation,
            None => Rotation::IDENTITY,
        };
        // Conjugate the world-axis rotation into the parent frame.
        let local = parent_rot
            .inverse()
            .then(Rotation::rot90(axis))
            .then(parent_rot);
        let node = self.get_mut(id)?;
        node.transform.rotation = local.then(node.transform.rotation);
        Ok(())
    }

    /// Number of shapes referencing `model_id` (in any frame).
    pub fn model_ref_count(&self, model_id: usize) -> usize {
        self.nodes
            .values()
            .filter(|n| n.frame_models().contains(&model_id))
            .count()
    }

    /// New shape referencing the same models, same parent and transform (*Reference*).
    pub fn instantiate(&mut self, shape: NodeId) -> Result<NodeId, SceneError> {
        let node = self.get(shape)?.clone();
        let model_id = node.model_id().ok_or(SceneError::NotAShape(shape))?;
        let parent = node.parent.unwrap_or(self.root);
        let id = self.add_shape(parent, model_id, node.transform)?;
        let new = self.get_mut(id)?;
        new.kind = node.kind;
        new.layer_id = node.layer_id;
        new.name = node.name;
        new.extra_transform_keys = node.extra_transform_keys;
        Ok(id)
    }

    /// Longest shape animation in the scene (at least 1 frame).
    pub fn animation_length(&self) -> u32 {
        self.nodes
            .values()
            .map(|n| n.frame_models().len() as u32)
            .max()
            .unwrap_or(1)
            .max(1)
    }

    fn shape_models_mut(&mut self, shape: NodeId) -> Result<&mut Vec<usize>, SceneError> {
        match &mut self.get_mut(shape)?.kind {
            NodeKind::Shape { models } => Ok(models),
            NodeKind::Group { .. } => Err(SceneError::NotAShape(shape)),
        }
    }

    /// Insert `model_id` as frame `index` of a shape (later frames shift by one).
    pub fn insert_frame(
        &mut self,
        shape: NodeId,
        index: usize,
        model_id: usize,
    ) -> Result<(), SceneError> {
        if model_id >= self.models.len() {
            return Err(SceneError::UnknownModel(model_id));
        }
        let models = self.shape_models_mut(shape)?;
        let index = index.min(models.len());
        models.insert(index, model_id);
        Ok(())
    }

    /// Remove frame `index` of a shape. The last remaining frame cannot be removed.
    /// Returns whether a frame was removed. Models are kept (see `compact_models`).
    pub fn remove_frame(&mut self, shape: NodeId, index: usize) -> Result<bool, SceneError> {
        let models = self.shape_models_mut(shape)?;
        if models.len() <= 1 || index >= models.len() {
            return Ok(false);
        }
        models.remove(index);
        Ok(true)
    }

    /// New shape with its own copy of the model (*Duplicate*).
    pub fn duplicate(&mut self, shape: NodeId) -> Result<NodeId, SceneError> {
        let id = self.instantiate(shape)?;
        self.convert_reference_to_duplicate(id)?;
        Ok(id)
    }

    /// `Ctrl+T`: give a shape its own copies of the models it shares with other
    /// shapes (every animation frame). Returns `true` if a copy was made.
    pub fn convert_reference_to_duplicate(&mut self, shape: NodeId) -> Result<bool, SceneError> {
        let models = self.shape_models_mut(shape)?.clone();
        let mut copies: BTreeMap<usize, usize> = BTreeMap::new();
        for &m in &models {
            if !copies.contains_key(&m) && self.model_ref_count(m) > 1 {
                let grid = self
                    .models
                    .get(m)
                    .ok_or(SceneError::UnknownModel(m))?
                    .clone();
                let new_id = self.add_model(grid);
                copies.insert(m, new_id);
            }
        }
        if copies.is_empty() {
            return Ok(false);
        }
        for m in self.shape_models_mut(shape)?.iter_mut() {
            if let Some(&c) = copies.get(m) {
                *m = c;
            }
        }
        Ok(true)
    }

    /// Rasterize the visible shapes under `ids` into one grid.
    /// Returns the grid and the world position of its voxel `(0, 0, 0)`.
    /// Later shapes overwrite earlier ones where they overlap.
    pub fn merge_to_grid(&self, ids: &[NodeId]) -> Result<(VoxelGrid, IVec3), SceneError> {
        let mut shapes = Vec::new();
        for &id in ids {
            self.get(id)?;
            shapes.extend(
                self.shapes_under(id)
                    .into_iter()
                    .filter(|&s| self.is_visible(s)),
            );
        }
        let mut bounds: Option<(IVec3, IVec3)> = None;
        for &s in &shapes {
            if let Some((lo, hi)) = self.world_bounds(s) {
                bounds = Some(match bounds {
                    Some((l, h)) => (l.min(lo), h.max(hi)),
                    None => (lo, hi),
                });
            }
        }
        let (lo, hi) = bounds.ok_or(SceneError::NothingToMerge)?;
        let size = hi - lo + IVec3::ONE;
        if size.max_element() > MAX_GRID_DIM as i32 {
            return Err(SceneError::TooLarge(size));
        }
        let mut grid = VoxelGrid::new(size.x as u32, size.y as u32, size.z as u32);
        for &s in &shapes {
            let model = self.model_of(s)?;
            let xf = self.shape_voxel_transform(s)?;
            let d = model.dimensions();
            for z in 0..d.z {
                for y in 0..d.y {
                    for x in 0..d.x {
                        let c = model.get(x, y, z);
                        if c != 0 {
                            let p = xf.apply(IVec3::new(x as i32, y as i32, z as i32)) - lo;
                            grid.set_i32(p.x, p.y, p.z, c);
                        }
                    }
                }
            }
        }
        Ok((grid, lo))
    }

    /// `U`: merge several objects into a single new shape (added to the root group),
    /// removing the originals. Returns the new shape id.
    pub fn union(&mut self, ids: &[NodeId]) -> Result<NodeId, SceneError> {
        let (grid, origin) = self.merge_to_grid(ids)?;
        let half = (grid.dimensions() / 2).as_ivec3();
        let model_id = self.add_model(grid);
        let root = self.root;
        let new_id = self.add_shape(root, model_id, Transform::from_translation(origin + half))?;
        for &id in ids {
            if id != self.root && self.nodes.contains_key(&id) {
                self.remove_node(id)?;
            }
        }
        Ok(new_id)
    }

    /// Topmost ancestor of `id` (or `id` itself) whose parent is `group`.
    /// This is the object a click selects while editing inside `group`.
    pub fn child_of_group_containing(&self, id: NodeId, group: NodeId) -> Option<NodeId> {
        let mut cur = id;
        loop {
            let parent = self.nodes.get(&cur)?.parent?;
            if parent == group {
                return Some(cur);
            }
            cur = parent;
        }
    }

    /// Whether `ancestor` is `id` or one of its ancestors.
    pub fn is_ancestor_or_self(&self, ancestor: NodeId, id: NodeId) -> bool {
        let mut cur = Some(id);
        while let Some(n) = cur {
            if n == ancestor {
                return true;
            }
            cur = self.nodes.get(&n).and_then(|node| node.parent);
        }
        false
    }

    /// Move `id` under `new_parent`, keeping its world placement.
    pub fn set_parent(&mut self, id: NodeId, new_parent: NodeId) -> Result<(), SceneError> {
        if id == self.root {
            return Err(SceneError::CannotRemoveRoot);
        }
        if !matches!(self.get(new_parent)?.kind, NodeKind::Group { .. })
            || self.is_ancestor_or_self(id, new_parent)
        {
            return Err(SceneError::NotAGroup(new_parent));
        }
        let world = self.world_transform(id);
        let parent_world = self.world_transform(new_parent);
        let old_parent = self.get(id)?.parent;
        if let Some(p) = old_parent {
            if let Some(NodeKind::Group { children }) = self.nodes.get_mut(&p).map(|n| &mut n.kind)
            {
                children.retain(|&c| c != id);
            }
        }
        if let NodeKind::Group { children } = &mut self.get_mut(new_parent)?.kind {
            children.push(id);
        }
        let node = self.get_mut(id)?;
        node.parent = Some(new_parent);
        node.transform = parent_world.inverse().compose(world);
        Ok(())
    }

    /// `Ctrl+R`: put nodes into a new group (created under the parent of the first one).
    pub fn group_nodes(&mut self, ids: &[NodeId]) -> Result<NodeId, SceneError> {
        let first = *ids.first().ok_or(SceneError::NothingToMerge)?;
        let parent = self.get(first)?.parent.unwrap_or(self.root);
        let group = self.add_group(parent, Transform::IDENTITY)?;
        for &id in ids {
            if self.nodes.contains_key(&id) && !self.is_ancestor_or_self(id, group) {
                self.set_parent(id, group)?;
            }
        }
        Ok(group)
    }

    /// `Ctrl+Shift+R`: move a group's children to its parent and delete the group.
    /// Returns the freed children.
    pub fn ungroup(&mut self, group: NodeId) -> Result<Vec<NodeId>, SceneError> {
        if group == self.root {
            return Err(SceneError::CannotRemoveRoot);
        }
        let node = self.get(group)?;
        let NodeKind::Group { children } = &node.kind else {
            return Err(SceneError::NotAGroup(group));
        };
        let children = children.clone();
        let parent = node.parent.unwrap_or(self.root);
        for &c in &children {
            self.set_parent(c, parent)?;
        }
        self.remove_node(group)?;
        Ok(children)
    }

    /// Everything except the models: used by the World Editor undo, which must never
    /// roll back voxel edits made in the Model Editor in the meantime.
    pub fn structure(&self) -> SceneStructure {
        SceneStructure {
            layers: self.layers.clone(),
            nodes: self.nodes.clone(),
            root: self.root,
            next_id: self.next_id,
        }
    }

    /// Restore a [`SceneStructure`]. Models are kept as they are now (World Editor
    /// operations only ever append models, so every referenced model still exists).
    pub fn restore_structure(&mut self, s: SceneStructure) {
        self.layers = s.layers;
        self.nodes = s.nodes;
        self.root = s.root;
        self.next_id = s.next_id;
    }

    /// Drop models no shape references and renumber the rest (keeping their order).
    /// Returns the mapping `old model id -> new model id` for the models that were kept.
    pub fn compact_models(&mut self) -> BTreeMap<usize, usize> {
        let mut used: Vec<bool> = vec![false; self.models.len()];
        for n in self.nodes.values() {
            for &m in n.frame_models() {
                if let Some(u) = used.get_mut(m) {
                    *u = true;
                }
            }
        }
        let mut remap = BTreeMap::new();
        let old = std::mem::take(&mut self.models);
        for (i, grid) in old.into_iter().enumerate() {
            if used[i] {
                remap.insert(i, self.models.len());
                self.models.push(grid);
            }
        }
        for n in self.nodes.values_mut() {
            if let NodeKind::Shape { models } = &mut n.kind {
                for m in models.iter_mut() {
                    *m = remap[m];
                }
            }
        }
        remap
    }
}

fn default_layers() -> Vec<SceneLayer> {
    (0..DEFAULT_LAYER_COUNT)
        .map(|i| SceneLayer {
            name: i.to_string(),
            color: [255, 255, 255],
            hidden: false,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cube(size: u32, color: u8) -> VoxelGrid {
        let mut g = VoxelGrid::new(size, size, size);
        g.full(color);
        g
    }

    #[test]
    fn rotation_vox_byte_roundtrip_and_algebra() {
        assert_eq!(Rotation::IDENTITY.to_vox_byte(), 4);
        assert_eq!(Rotation::from_vox_byte(4), Rotation::IDENTITY);
        // Invalid byte (same column twice) decodes to identity.
        assert_eq!(Rotation::from_vox_byte(0), Rotation::IDENTITY);

        let rz = Rotation::rot90(Axis::Z);
        assert_eq!(rz.apply(IVec3::new(1, 0, 0)), IVec3::new(0, 1, 0));
        assert_eq!(rz.then(rz).then(rz).then(rz), Rotation::IDENTITY);
        assert_eq!(rz.then(rz.inverse()), Rotation::IDENTITY);

        // Every one of the 48 signed permutations survives the byte encoding.
        let mut count = 0;
        for b in 0u8..128 {
            let r = Rotation::from_vox_byte(b);
            if r.to_vox_byte() == b {
                count += 1;
                let v = IVec3::new(1, 2, 3);
                assert_eq!(r.inverse().apply(r.apply(v)), v);
            }
        }
        assert_eq!(count, 48);
    }

    #[test]
    fn world_bounds_follow_group_transforms() {
        let mut scene = Scene::new();
        let m = scene.add_model(cube(4, 1));
        let root = scene.root();
        let g = scene
            .add_group(root, Transform::from_translation(IVec3::new(10, 0, 0)))
            .unwrap();
        let s = scene
            .add_shape(g, m, Transform::from_translation(IVec3::new(0, 0, 5)))
            .unwrap();
        // size 4 → local voxels [-2, 1], shifted by (10, 0, 5).
        assert_eq!(
            scene.world_bounds(s),
            Some((IVec3::new(8, -2, 3), IVec3::new(11, 1, 6)))
        );
        assert_eq!(scene.world_bounds(g), scene.world_bounds(s));
    }

    #[test]
    fn move_to_ground_and_rotate() {
        let mut scene = Scene::new();
        let m = scene.add_model(VoxelGrid::new(2, 2, 6));
        let root = scene.root();
        let s = scene
            .add_shape(root, m, Transform::from_translation(IVec3::new(0, 0, 20)))
            .unwrap();
        scene.move_to_ground(s).unwrap();
        assert_eq!(scene.world_bounds(s).unwrap().0.z, 0);

        // Lying the tall object down around X swaps its Y and Z extents.
        scene.rotate90(s, Axis::X).unwrap();
        let (lo, hi) = scene.world_bounds(s).unwrap();
        assert_eq!(hi - lo + IVec3::ONE, IVec3::new(2, 6, 2));
    }

    #[test]
    fn rotate_inside_rotated_group_uses_world_axis() {
        let mut scene = Scene::new();
        let m = scene.add_model(VoxelGrid::new(2, 4, 6));
        let root = scene.root();
        let g = scene
            .add_group(
                root,
                Transform {
                    rotation: Rotation::rot90(Axis::X),
                    translation: IVec3::ZERO,
                },
            )
            .unwrap();
        let s = scene.add_shape(g, m, Transform::IDENTITY).unwrap();
        let before = scene.world_transform(s).rotation;
        scene.rotate90(s, Axis::Z).unwrap();
        let after = scene.world_transform(s).rotation;
        assert_eq!(after, Rotation::rot90(Axis::Z).then(before));
    }

    #[test]
    fn instances_share_models_until_converted() {
        let mut scene = Scene::single(cube(3, 2));
        let s0 = scene.shapes_under(scene.root())[0];
        let s1 = scene.instantiate(s0).unwrap();
        assert_eq!(scene.node(s1).unwrap().model_id(), Some(0));
        assert_eq!(scene.model_ref_count(0), 2);

        assert!(scene.convert_reference_to_duplicate(s1).unwrap());
        assert_eq!(scene.node(s1).unwrap().model_id(), Some(1));
        assert_eq!(scene.models.len(), 2);
        // Already unique: no extra copy.
        assert!(!scene.convert_reference_to_duplicate(s1).unwrap());

        let s2 = scene.duplicate(s0).unwrap();
        assert_eq!(scene.node(s2).unwrap().model_id(), Some(2));
    }

    #[test]
    fn union_merges_objects_in_place() {
        let mut scene = Scene::new();
        let a = scene.add_model(cube(2, 1));
        let b = scene.add_model(cube(2, 2));
        let root = scene.root();
        let sa = scene
            .add_shape(root, a, Transform::from_translation(IVec3::new(1, 1, 1)))
            .unwrap();
        let sb = scene
            .add_shape(root, b, Transform::from_translation(IVec3::new(5, 1, 1)))
            .unwrap();
        let bounds_before = (
            scene.world_bounds(sa).unwrap().0,
            scene.world_bounds(sb).unwrap().1,
        );

        let u = scene.union(&[sa, sb]).unwrap();
        assert!(scene.node(sa).is_none() && scene.node(sb).is_none());
        assert_eq!(scene.world_bounds(u), Some(bounds_before));

        let model = &scene.models[scene.node(u).unwrap().model_id().unwrap()];
        assert_eq!(model.dimensions().to_array(), [6, 2, 2]);
        assert_eq!(model.count_solid(), 16);
        assert_eq!(model.get(0, 0, 0), 1);
        assert_eq!(model.get(5, 0, 0), 2);

        scene.compact_models();
        assert_eq!(scene.models.len(), 1);
        assert_eq!(scene.node(u).unwrap().model_id(), Some(0));
    }

    #[test]
    fn hidden_layer_or_ancestor_hides_node() {
        let mut scene = Scene::single(cube(2, 1));
        let root = scene.root();
        let s = scene.shapes_under(root)[0];
        assert!(scene.is_visible(s));
        scene.layers[0].hidden = true;
        assert!(!scene.is_visible(s));
        scene.layers[0].hidden = false;
        scene.node_mut(root).unwrap().hidden = true;
        assert!(!scene.is_visible(s));
        assert_eq!(
            scene.merge_to_grid(&[root]),
            Err(SceneError::NothingToMerge)
        );
    }

    #[test]
    fn remove_node_removes_subtree() {
        let mut scene = Scene::new();
        let m = scene.add_model(cube(1, 1));
        let root = scene.root();
        let g = scene.add_group(root, Transform::IDENTITY).unwrap();
        scene.add_shape(g, m, Transform::IDENTITY).unwrap();
        scene.add_shape(g, m, Transform::IDENTITY).unwrap();
        assert_eq!(scene.node_count(), 4);
        scene.remove_node(g).unwrap();
        assert_eq!(scene.node_count(), 1);
        assert!(scene.node(root).unwrap().children().is_empty());
        assert_eq!(scene.remove_node(root), Err(SceneError::CannotRemoveRoot));
    }

    #[test]
    fn group_and_ungroup_keep_world_placement() {
        let mut scene = Scene::new();
        let m = scene.add_model(VoxelGrid::new(2, 3, 4));
        let root = scene.root();
        let rotated = scene
            .add_group(
                root,
                Transform {
                    rotation: Rotation::rot90(Axis::Z),
                    translation: IVec3::new(5, 0, 0),
                },
            )
            .unwrap();
        let a = scene
            .add_shape(rotated, m, Transform::from_translation(IVec3::new(1, 2, 3)))
            .unwrap();
        let b = scene
            .add_shape(root, m, Transform::from_translation(IVec3::new(-7, 0, 0)))
            .unwrap();
        let before = (scene.world_bounds(a), scene.world_bounds(b));

        let g = scene.group_nodes(&[a, b]).unwrap();
        assert_eq!(scene.node(a).unwrap().parent, Some(g));
        assert_eq!(scene.node(b).unwrap().parent, Some(g));
        assert_eq!((scene.world_bounds(a), scene.world_bounds(b)), before);
        // The new group lives under the first node's parent.
        assert_eq!(scene.node(g).unwrap().parent, Some(rotated));
        assert_eq!(scene.node(rotated).unwrap().children(), &[g]);

        assert_eq!(scene.child_of_group_containing(a, root), Some(rotated));
        assert_eq!(scene.child_of_group_containing(a, g), Some(a));

        let freed = scene.ungroup(g).unwrap();
        assert_eq!(freed, vec![a, b]);
        assert!(scene.node(g).is_none());
        assert_eq!(scene.node(b).unwrap().parent, Some(rotated));
        assert_eq!((scene.world_bounds(a), scene.world_bounds(b)), before);

        // A group cannot be moved inside itself.
        let g2 = scene.group_nodes(&[a]).unwrap();
        assert!(scene.set_parent(g2, g2).is_err());
    }

    #[test]
    fn structure_snapshot_keeps_current_models() {
        let mut scene = Scene::single(cube(2, 1));
        let snap = scene.structure();
        let s = scene.shapes_under(scene.root())[0];
        let copy = scene.duplicate(s).unwrap();
        scene.models[0].set(0, 0, 0, 9); // voxel edit after the snapshot
        scene.restore_structure(snap);
        assert!(scene.node(copy).is_none());
        assert_eq!(scene.models[0].get(0, 0, 0), 9, "voxel edits survive");
    }

    #[test]
    fn rotation_apply_f32_matches_integer() {
        for b in 0u8..128 {
            let r = Rotation::from_vox_byte(b);
            let v = IVec3::new(3, -5, 7);
            assert_eq!(r.apply_f32(v.as_vec3()), r.apply(v).as_vec3());
        }
    }

    #[test]
    fn animated_shape_frames_follow_current_frame() {
        let mut scene = Scene::new();
        let small = scene.add_model(cube(2, 1));
        let big = scene.add_model(cube(4, 2));
        let root = scene.root();
        let s = scene.add_shape(root, small, Transform::IDENTITY).unwrap();
        scene.insert_frame(s, 1, big).unwrap();
        let other = scene.add_shape(root, small, Transform::IDENTITY).unwrap();
        assert_eq!(scene.animation_length(), 2);

        scene.current_frame = 1;
        assert_eq!(scene.shape_model(s), Some(big));
        assert_eq!(
            scene.shape_model(other),
            Some(small),
            "static shape on every frame"
        );
        let (lo, hi) = scene.world_bounds(s).unwrap();
        assert_eq!(hi - lo + IVec3::ONE, IVec3::splat(4));
        scene.current_frame = 2; // loops
        assert_eq!(scene.shape_model(s), Some(small));

        // Instances share every frame; Ctrl+T copies each shared frame model.
        let inst = scene.instantiate(s).unwrap();
        assert_eq!(scene.node(inst).unwrap().frame_models(), &[small, big]);
        assert!(scene.convert_reference_to_duplicate(inst).unwrap());
        let copied = scene.node(inst).unwrap().frame_models().to_vec();
        assert_eq!(copied.len(), 2);
        assert!(!copied.contains(&small) && !copied.contains(&big));

        assert!(scene.remove_frame(s, 0).unwrap());
        assert_eq!(scene.node(s).unwrap().frame_models(), &[big]);
        assert!(
            !scene.remove_frame(s, 0).unwrap(),
            "keeps at least one frame"
        );
    }
}
