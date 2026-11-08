#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaterialType {
    Diffuse,
    Metal,
    Glass,
    Emit,
    Blend,
    Cloud,
}

impl MaterialType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Diffuse => "_diffuse",
            Self::Metal => "_metal",
            Self::Glass => "_glass",
            Self::Emit => "_emit",
            Self::Blend => "_blend",
            Self::Cloud => "_media",
        }
    }

    pub fn from_vox_str(s: &str) -> Self {
        match s {
            "_metal" => Self::Metal,
            "_glass" => Self::Glass,
            "_emit" => Self::Emit,
            "_blend" => Self::Blend,
            "_media" | "_cloud" => Self::Cloud,
            _ => Self::Diffuse,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Material {
    pub id: u8,
    pub kind: MaterialType,
    pub weight: f32,
    pub rough: f32,
    pub spec: f32,
    pub ior: f32,
    pub att: f32,
    pub flux: f32,
    pub emit: f32,
    pub ldr: f32,
    pub metal: f32,
    pub alpha: f32,
    pub density: f32,
}

impl Default for Material {
    fn default() -> Self {
        Self {
            id: 1,
            kind: MaterialType::Diffuse,
            weight: 1.0,
            rough: 0.1,
            spec: 0.5,
            ior: 1.3,
            att: 0.0,
            flux: 0.0,
            emit: 0.0,
            ldr: 0.0,
            metal: 0.0,
            alpha: 1.0,
            density: 0.0,
        }
    }
}

impl Material {
    pub fn default_table() -> [Material; 256] {
        std::array::from_fn(|i| Material {
            id: i as u8,
            ..Default::default()
        })
    }
}

