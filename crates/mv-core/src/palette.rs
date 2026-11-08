#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba {
    pub const TRANSPARENT: Self = Self { r: 0, g: 0, b: 0, a: 0 };
    pub const WHITE: Self = Self { r: 255, g: 255, b: 255, a: 255 };

    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    pub const fn from_u32_abgr(v: u32) -> Self {
        Self {
            r: (v & 0xFF) as u8,
            g: ((v >> 8) & 0xFF) as u8,
            b: ((v >> 16) & 0xFF) as u8,
            a: ((v >> 24) & 0xFF) as u8,
        }
    }

    pub const fn to_u32_abgr(self) -> u32 {
        (self.r as u32)
            | ((self.g as u32) << 8)
            | ((self.b as u32) << 16)
            | ((self.a as u32) << 24)
    }

    pub fn to_hsv(self) -> (f32, f32, f32) {
        let r = self.r as f32 / 255.0;
        let g = self.g as f32 / 255.0;
        let b = self.b as f32 / 255.0;
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let delta = max - min;

        let h = if delta <= 1e-6 {
            0.0
        } else if (max - r).abs() <= 1e-6 {
            60.0 * (((g - b) / delta).rem_euclid(6.0))
        } else if (max - g).abs() <= 1e-6 {
            60.0 * (((b - r) / delta) + 2.0)
        } else {
            60.0 * (((r - g) / delta) + 4.0)
        };
        let s = if max <= 1e-6 { 0.0 } else { delta / max };
        (h, s, max)
    }

    pub fn lerp(self, other: Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        Self {
            r: (self.r as f32 + (other.r as f32 - self.r as f32) * t).round() as u8,
            g: (self.g as f32 + (other.g as f32 - self.g as f32) * t).round() as u8,
            b: (self.b as f32 + (other.b as f32 - self.b as f32) * t).round() as u8,
            a: (self.a as f32 + (other.a as f32 - self.a as f32) * t).round() as u8,
        }
    }
}

/// 256-color palette (index 0 is empty space, indices 1..=255 are usable swatches).
#[derive(Debug, Clone)]
pub struct Palette {
    pub colors: [Rgba; 256],
    pub selected: u8,
    pub notes: [String; 32],
}

impl Default for Palette {
    fn default() -> Self {
        Self::default_mv_palette()
    }
}

impl Palette {
    pub fn default_mv_palette() -> Self {
        let mut colors = [Rgba::WHITE; 256];
        colors[0] = Rgba::TRANSPARENT;

        // Classic 255-color MagicaVoxel cube + ramps (6x6x6 RGB cube + R/G/B/Grayscale ramps)
        let mut idx = 1usize;
        let levels = [255u8, 204, 153, 102, 51, 0];
        for &r in &levels {
            for &g in &levels {
                for &b in &levels {
                    if r == 0 && g == 0 && b == 0 {
                        continue;
                    }
                    if idx < 256 {
                        colors[idx] = Rgba::new(r, g, b, 255);
                        idx += 1;
                    }
                }
            }
        }
        let ramp = [238u8, 221, 187, 170, 136, 119, 85, 68, 34, 17];
        for &v in &ramp {
            if idx < 256 {
                colors[idx] = Rgba::new(v, 0, 0, 255);
                idx += 1;
            }
        }
        for &v in &ramp {
            if idx < 256 {
                colors[idx] = Rgba::new(0, v, 0, 255);
                idx += 1;
            }
        }
        for &v in &ramp {
            if idx < 256 {
                colors[idx] = Rgba::new(0, 0, v, 255);
                idx += 1;
            }
        }
        for &v in &ramp {
            if idx < 256 {
                colors[idx] = Rgba::new(v, v, v, 255);
                idx += 1;
            }
        }
        colors[255] = Rgba::new(20, 20, 20, 255);

        Self {
            colors,
            selected: 1,
            notes: std::array::from_fn(|i| format!("Row {}", i)),
        }
    }

    /// Generate grey palette (`pal bw`)
    pub fn generate_bw(&mut self) {
        for i in 1..=255usize {
            let v = (255 - (i - 1)) as u8;
            self.colors[i] = Rgba::new(v, v, v, 255);
        }
    }

    /// Generate default Mac/MV palette (`pal mac`)
    pub fn generate_mac(&mut self) {
        let default = Self::default_mv_palette();
        self.colors = default.colors;
    }

    /// Fill entire palette with the chosen color index (`pal fill [index]`)
    pub fn fill_from_index(&mut self, index: u8) {
        let c = self.colors[index as usize];
        for i in 1..=255 {
            self.colors[i] = c;
        }
    }

    /// Mask out all unused color swatches (`pal mask`)
    pub fn mask_unused(&mut self, used: &[bool; 256]) {
        for i in 1..=255 {
            if !used[i] {
                self.colors[i] = Rgba::new(35, 35, 35, 255);
            }
        }
    }

    /// Swap two swatches (`CTRL+DRAG`)
    pub fn swap(&mut self, a: u8, b: u8) {
        if a != 0 && b != 0 {
            self.colors.swap(a as usize, b as usize);
        }
    }

    /// Duplicate color from `src` to `dst` (`CTRL+SHIFT+DRAG`)
    pub fn duplicate(&mut self, src: u8, dst: u8) {
        if src != 0 && dst != 0 {
            self.colors[dst as usize] = self.colors[src as usize];
        }
    }

    /// Fill gradient between two indices (`ALT+SHIFT+DRAG`)
    pub fn fill_gradient(&mut self, start: u8, end: u8) {
        if start == 0 || end == 0 || start == end {
            return;
        }
        let (lo, hi) = if start < end { (start, end) } else { (end, start) };
        let c_lo = self.colors[lo as usize];
        let c_hi = self.colors[hi as usize];
        let span = (hi - lo) as f32;
        for i in lo..=hi {
            let t = (i - lo) as f32 / span;
            self.colors[i as usize] = c_lo.lerp(c_hi, t);
        }
    }

    /// Sort palette colors (`pal sort [+-hsvrgb]`)
    pub fn sort_by_spec(&mut self, spec: &str) {
        let mut descending = false;
        let mut keys = Vec::new();
        for ch in spec.chars() {
            match ch {
                '-' => descending = true,
                '+' => descending = false,
                'h' | 'H' | 's' | 'S' | 'v' | 'V' | 'r' | 'R' | 'g' | 'G' | 'b' | 'B' => {
                    keys.push(ch.to_ascii_lowercase());
                }
                _ => {}
            }
        }
        if keys.is_empty() {
            keys.extend(['h', 's', 'v']);
        }
        let slice = &mut self.colors[1..=255];
        slice.sort_by(|a, b| {
            let (ah, as_, av) = a.to_hsv();
            let (bh, bs, bv) = b.to_hsv();
            for &k in &keys {
                let ord = match k {
                    'h' => ah.total_cmp(&bh),
                    's' => as_.total_cmp(&bs),
                    'v' => av.total_cmp(&bv),
                    'r' => a.r.cmp(&b.r),
                    'g' => a.g.cmp(&b.g),
                    'b' => a.b.cmp(&b.b),
                    _ => std::cmp::Ordering::Equal,
                };
                if ord != std::cmp::Ordering::Equal {
                    return if descending { ord.reverse() } else { ord };
                }
            }
            std::cmp::Ordering::Equal
        });
    }
}

