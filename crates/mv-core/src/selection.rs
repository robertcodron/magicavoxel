use glam::IVec3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectionMask {
    size_x: u32,
    size_y: u32,
    size_z: u32,
    mask: Vec<bool>,
}

impl SelectionMask {
    pub fn new(size_x: u32, size_y: u32, size_z: u32) -> Self {
        let len = (size_x * size_y * size_z) as usize;
        Self {
            size_x,
            size_y,
            size_z,
            mask: vec![false; len],
        }
    }

    pub fn resize(&mut self, size_x: u32, size_y: u32, size_z: u32) {
        self.size_x = size_x;
        self.size_y = size_y;
        self.size_z = size_z;
        self.mask.clear();
        self.mask.resize((size_x * size_y * size_z) as usize, false);
    }

    #[inline]
    fn index(&self, x: u32, y: u32, z: u32) -> Option<usize> {
        if x < self.size_x && y < self.size_y && z < self.size_z {
            Some((x + y * self.size_x + z * self.size_x * self.size_y) as usize)
        } else {
            None
        }
    }

    #[inline]
    pub fn get(&self, x: u32, y: u32, z: u32) -> bool {
        self.index(x, y, z).map(|i| self.mask[i]).unwrap_or(false)
    }

    #[inline]
    pub fn set(&mut self, x: u32, y: u32, z: u32, selected: bool) {
        if let Some(i) = self.index(x, y, z) {
            self.mask[i] = selected;
        }
    }

    pub fn any_selected(&self) -> bool {
        self.mask.iter().any(|&b| b)
    }

    pub fn count_selected(&self) -> usize {
        self.mask.iter().filter(|&&b| b).count()
    }

    pub fn clear(&mut self) {
        self.mask.fill(false);
    }

    pub fn select_all_occupied(&mut self, voxels: &[u8]) {
        for (s, &v) in self.mask.iter_mut().zip(voxels.iter()) {
            *s = v != 0;
        }
    }

    pub fn invert_occupied(&mut self, voxels: &[u8]) {
        for (s, &v) in self.mask.iter_mut().zip(voxels.iter()) {
            if v != 0 {
                *s = !*s;
            } else {
                *s = false;
            }
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct VoxelClipboard {
    pub voxels: Vec<(IVec3, u8)>,
}

impl VoxelClipboard {
    pub fn is_empty(&self) -> bool {
        self.voxels.is_empty()
    }
}

