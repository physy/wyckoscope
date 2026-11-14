use bevy::prelude::{Mat3, Vec3};

#[derive(Debug, Clone)]
pub struct AtomSite {
    pub element: String,
    pub label: String,
    pub fractional: Vec3,
    pub occupancy: f32,
    /// Rotational part of the symmetry operation that generated this atom.
    /// This is used to transform axial vectors attached to a crystallographic site.
    pub symmetry_rotation: Mat3,
    /// Magnetic moment in the crystallographic Cartesian frame, in μB when read from mCIF.
    pub magnetic_moment: Option<Vec3>,
}

#[derive(Debug, Clone)]
pub struct UnitCell {
    pub a: f32,
    pub b: f32,
    pub c: f32,
    pub alpha: f32,
    pub beta: f32,
    pub gamma: f32,
    pub vectors: [Vec3; 3],
}

impl UnitCell {
    pub fn new(a: f32, b: f32, c: f32, alpha: f32, beta: f32, gamma: f32) -> Self {
        let alpha_r = alpha.to_radians();
        let beta_r = beta.to_radians();
        let gamma_r = gamma.to_radians();
        let sin_gamma = gamma_r.sin().abs().max(1.0e-6);

        let va = Vec3::new(a, 0.0, 0.0);
        let vb = Vec3::new(b * gamma_r.cos(), b * gamma_r.sin(), 0.0);
        let cx = c * beta_r.cos();
        let cy = c * (alpha_r.cos() - beta_r.cos() * gamma_r.cos()) / sin_gamma;
        let cz = (c * c - cx * cx - cy * cy).max(0.0).sqrt();
        let vc = Vec3::new(cx, cy, cz);

        Self {
            a,
            b,
            c,
            alpha,
            beta,
            gamma,
            vectors: [va, vb, vc],
        }
    }

    pub fn fractional_to_cartesian(&self, p: Vec3) -> Vec3 {
        self.vectors[0] * p.x + self.vectors[1] * p.y + self.vectors[2] * p.z
    }

    pub fn symmetry_to_cartesian(&self, operation: Mat3) -> Mat3 {
        let basis = Mat3::from_cols(self.vectors[0], self.vectors[1], self.vectors[2]);
        basis * operation * basis.inverse()
    }

    pub fn crystal_axes_to_cartesian(&self, components: Vec3) -> Vec3 {
        let axes = self.vectors.map(Vec3::normalize_or_zero);
        axes[0] * components.x + axes[1] * components.y + axes[2] * components.z
    }

    pub fn magcif_cartesian_to_cartesian(&self, components: Vec3) -> Vec3 {
        let x = self.vectors[0].normalize_or_zero();
        let z = self.vectors[0].cross(self.vectors[1]).normalize_or_zero();
        let y = z.cross(x).normalize_or_zero();
        x * components.x + y * components.y + z * components.z
    }

    pub fn volume(&self) -> f32 {
        self.vectors[0]
            .dot(self.vectors[1].cross(self.vectors[2]))
            .abs()
    }
}

#[derive(Debug, Clone)]
pub struct CrystalStructure {
    pub title: String,
    pub cell: UnitCell,
    /// Sites as written in the source file, before symmetry expansion.
    pub asymmetric_atoms: Vec<AtomSite>,
    /// Sites expanded with the available space-group operations.
    pub atoms: Vec<AtomSite>,
    pub space_group: Option<String>,
    pub symmetry_operation_count: usize,
    pub source_format: SourceFormat,
    pub warnings: Vec<String>,
    /// Non-structural settings imported from a source format such as VESTA.
    pub imported_visuals: ImportedVisuals,
}

#[derive(Debug, Clone, Default)]
pub struct ImportedVisuals {
    pub background: Option<[u8; 3]>,
    pub bond_rules: Vec<ImportedBondRule>,
    pub vectors: Vec<ImportedVector>,
}

#[derive(Debug, Clone)]
pub struct ImportedBondRule {
    pub center: String,
    pub neighbor: String,
    pub min_length: f32,
    pub max_length: f32,
    pub show_polyhedra: bool,
}

#[derive(Debug, Clone)]
pub struct ImportedVector {
    pub label: String,
    /// VESTA's VECTR components in its Cartesian crystallographic frame.
    pub components: Vec3,
    pub color: [u8; 3],
    pub scale: f32,
    pub penetrate_atom: bool,
    pub add_atom_radius: bool,
}

impl CrystalStructure {
    pub fn visible_atoms(&self, expand_symmetry: bool) -> &[AtomSite] {
        if expand_symmetry {
            &self.atoms
        } else {
            &self.asymmetric_atoms
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceFormat {
    Cif,
    Mcif,
    Vesta,
}

impl SourceFormat {
    pub fn label(self) -> &'static str {
        match self {
            Self::Cif => "CIF",
            Self::Mcif => "mCIF",
            Self::Vesta => "VESTA",
        }
    }
}
