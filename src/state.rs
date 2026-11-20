use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Mutex, mpsc},
};

use bevy::prelude::*;
use bevy_egui::egui;
use serde::{Deserialize, Serialize};
use wyckoscope::{
    elements::ElementColorScheme,
    model::{CrystalStructure, SiteId},
};

#[derive(Resource)]
pub(crate) struct AppState {
    pub(crate) structure: CrystalStructure,
    pub(crate) file_name: String,
    pub(crate) error: Option<String>,
}

/// Candidates parsed from one multi-block CIF, awaiting the user's choice.
#[derive(Resource, Default)]
pub(crate) struct StructureSelection {
    pub(crate) file_name: String,
    pub(crate) candidates: Vec<wyckoscope::parser::StructureCandidate>,
    pub(crate) selected: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum InspectorTab {
    File,
    Structure,
    Objects,
    Vectors,
    Bonds,
    View,
    Lighting,
    Elements,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum SceneObjectAnchor {
    World,
    Crystal,
    /// Local coordinates are expressed in fractional lattice units relative
    /// to this asymmetric crystallographic site.
    Atom {
        site_id: SiteId,
    },
}

impl SceneObjectAnchor {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::World => "World",
            Self::Crystal => "Crystal",
            Self::Atom { .. } => "Atom",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) enum SceneObjectKind {
    Ground {
        size: f32,
        metallic: f32,
        roughness: f32,
    },
    Label {
        text: String,
        font_size: f32,
        show_background: bool,
        background_color: [u8; 3],
        background_opacity: f32,
        show_outline: bool,
        outline_color: [u8; 3],
        outline_width: f32,
    },
    CrystalPlane {
        miller: [i32; 3],
        offset: f32,
        size: f32,
        show_normal: bool,
        normal_length: f32,
    },
    Arrow {
        direction: ArrowDirection,
        shaft_radius: f32,
        head_length: f32,
        head_radius: f32,
    },
    Marker {
        radius: f32,
    },
    Sphere {
        radius: f32,
    },
    Box {
        size: [f32; 3],
    },
    Cylinder {
        radius: f32,
        height: f32,
    },
    Cone {
        radius: f32,
        height: f32,
    },
    BoundingBox {
        size: [f32; 3],
        line_radius: f32,
    },
    Line {
        start: [f32; 3],
        end: [f32; 3],
        radius: f32,
    },
    Polyline {
        points: Vec<[f32; 3]>,
        radius: f32,
    },
    Measurement {
        kind: MeasurementKind,
        points: Vec<MeasurementPoint>,
        radius: f32,
        label_size: f32,
        show_background: bool,
        background_color: [u8; 3],
        background_opacity: f32,
        show_outline: bool,
        outline_color: [u8; 3],
        outline_width: f32,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub(crate) enum ArrowDirection {
    Cartesian([f32; 3]),
    CrystalDirection([i32; 3]),
    PlaneNormal([i32; 3]),
}

impl ArrowDirection {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Cartesian(_) => "Cartesian (x y z)",
            Self::CrystalDirection(_) => "Crystal direction [u v w]",
            Self::PlaneNormal(_) => "Plane normal (h k l)",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum MeasurementKind {
    Distance,
    Angle,
    Dihedral,
}

impl MeasurementKind {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Distance => "Distance",
            Self::Angle => "Angle",
            Self::Dihedral => "Dihedral",
        }
    }

    pub(crate) fn point_count(self) -> usize {
        match self {
            Self::Distance => 2,
            Self::Angle => 3,
            Self::Dihedral => 4,
        }
    }
}

/// Stable source identity plus its selected periodic image. Keeping the
/// fractional coordinate preserves measurements across scene rebuilds.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub(crate) struct MeasurementPoint {
    pub(crate) site_id: SiteId,
    pub(crate) fractional: [f32; 3],
}

impl SceneObjectKind {
    pub(crate) fn label(&self) -> &'static str {
        match self {
            Self::Ground { .. } => "Ground",
            Self::Label { .. } => "Label",
            Self::CrystalPlane { .. } => "Crystal plane",
            Self::Arrow { .. } => "Arrow",
            Self::Marker { .. } => "Marker",
            Self::Sphere { .. } => "Sphere",
            Self::Box { .. } => "Box",
            Self::Cylinder { .. } => "Cylinder",
            Self::Cone { .. } => "Cone",
            Self::BoundingBox { .. } => "Bounding box",
            Self::Line { .. } => "Line",
            Self::Polyline { .. } => "Polyline",
            Self::Measurement { kind, .. } => kind.label(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct SceneObject {
    pub(crate) id: u64,
    pub(crate) name: String,
    pub(crate) visible: bool,
    pub(crate) anchor: SceneObjectAnchor,
    pub(crate) position: [f32; 3],
    pub(crate) rotation: [f32; 3],
    pub(crate) color: [u8; 3],
    pub(crate) kind: SceneObjectKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub(crate) struct SceneObjects {
    pub(crate) objects: Vec<SceneObject>,
    pub(crate) next_id: u64,
    #[serde(skip)]
    pub(crate) selected: Option<u64>,
}

impl SceneObjects {
    pub(crate) fn add(&mut self, kind: SceneObjectKind) {
        let id = self.next_id;
        self.next_id += 1;
        let name = kind.label().to_string();
        self.objects.push(SceneObject {
            id,
            name,
            visible: true,
            anchor: SceneObjectAnchor::World,
            position: [0.0; 3],
            rotation: [0.0; 3],
            color: [20, 20, 20],
            kind,
        });
        self.selected = Some(id);
    }

    pub(crate) fn add_anchored(&mut self, kind: SceneObjectKind, anchor: SceneObjectAnchor) {
        self.add(kind);
        if let Some(object) = self.objects.last_mut() {
            object.anchor = anchor;
        }
    }
}

#[derive(Resource)]
pub(crate) struct InspectorState {
    pub(crate) tab: InspectorTab,
}

impl Default for InspectorState {
    fn default() -> Self {
        Self {
            tab: InspectorTab::File,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum AtomDisplayMode {
    BallAndStick,
    SpaceFilling,
    Wireframe,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum BondMatchMode {
    Element,
    SiteLabel,
}

impl BondMatchMode {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Element => "Elements",
            Self::SiteLabel => "Site labels",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum BondSearchMode {
    A2BondedToA1,
    AtomsBondedToA1,
    Molecules,
}

impl BondSearchMode {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::A2BondedToA1 => "Search A2 bonded to A1",
            Self::AtomsBondedToA1 => "Search atoms bonded to A1",
            Self::Molecules => "Search molecules",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum BondBoundaryMode {
    DoNotSearchBeyond,
    SearchAdditionalForA1,
    SearchRecursivelyIfEitherVisible,
}

impl BondBoundaryMode {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::DoNotSearchBeyond => "Do not search atoms beyond the boundary",
            Self::SearchAdditionalForA1 => {
                "Search additional atoms if A1 is included in the boundary"
            }
            Self::SearchRecursivelyIfEitherVisible => {
                "Search additional atoms recursively if either A1 or A2 is visible"
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum BondStyle {
    UnicolorCylinder,
    BicolorCylinder,
    ColorLine,
    GradientLine,
    DottedLine,
    DashedLine,
}

impl BondStyle {
    pub(crate) const ALL: [Self; 6] = [
        Self::UnicolorCylinder,
        Self::BicolorCylinder,
        Self::ColorLine,
        Self::GradientLine,
        Self::DottedLine,
        Self::DashedLine,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::UnicolorCylinder => "Unicolor cylinder",
            Self::BicolorCylinder => "Bicolor cylinder",
            Self::ColorLine => "Color line",
            Self::GradientLine => "Gradient line",
            Self::DottedLine => "Dotted line",
            Self::DashedLine => "Dashed line",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct BondRule {
    pub(crate) enabled: bool,
    pub(crate) match_mode: BondMatchMode,
    pub(crate) search_mode: BondSearchMode,
    pub(crate) boundary_mode: BondBoundaryMode,
    pub(crate) center: String,
    pub(crate) neighbor: String,
    pub(crate) min_length: f32,
    pub(crate) max_length: f32,
    pub(crate) bond_style: BondStyle,
    pub(crate) bond_radius: f32,
    pub(crate) bond_pixel_width: f32,
    pub(crate) bond_color: [u8; 3],
    pub(crate) show_polyhedra: bool,
    pub(crate) polyhedron_color: [u8; 3],
    pub(crate) polyhedron_opacity: f32,
}

impl Default for BondRule {
    fn default() -> Self {
        Self {
            enabled: true,
            match_mode: BondMatchMode::Element,
            search_mode: BondSearchMode::A2BondedToA1,
            boundary_mode: BondBoundaryMode::SearchAdditionalForA1,
            center: String::new(),
            neighbor: String::new(),
            min_length: 0.0,
            max_length: 2.5,
            bond_style: BondStyle::BicolorCylinder,
            bond_radius: 0.095,
            bond_pixel_width: 2.0,
            bond_color: [127, 127, 127],
            show_polyhedra: true,
            polyhedron_color: [74, 164, 255],
            polyhedron_opacity: 0.50,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum VectorCoordinates {
    Lattice,
    Cartesian,
}

impl VectorCoordinates {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Lattice => "Lattice [u v w]",
            Self::Cartesian => "Cartesian (x, y, z)",
        }
    }
}

/// VESTA-compatible symmetry behavior for vectors attached to a site.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum VectorKind {
    Axial,
    Polar,
}

impl VectorKind {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Axial => "Axial vector",
            Self::Polar => "Polar vector",
        }
    }
}

/// The atom(s) a vector is attached to. All vectors use this same attachment
/// model, regardless of whether their initial values came from an mCIF, VESTA,
/// or the user.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) enum VectorTarget {
    Site { site_id: SiteId },
    IndividualAtom { site_id: SiteId, fractional: Vec3 },
}

impl VectorTarget {
    pub(crate) fn label(&self) -> &'static str {
        match self {
            Self::Site { .. } => "Site vector",
            Self::IndividualAtom { .. } => "Atom vector",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct VectorStyle {
    pub(crate) color: [u8; 3],
    /// Multiplier for the physical vector length.
    pub(crate) scale: f32,
    /// Radius of the arrow shaft in Å.
    pub(crate) shaft_radius: f32,
    /// Multiplier for the automatically sized arrow head length.
    pub(crate) head_length: f32,
    /// Multiplier for the arrow head diameter.
    pub(crate) head_radius: f32,
    pub(crate) penetrate_atom: bool,
    pub(crate) add_atom_radius: bool,
}

impl VectorStyle {
    fn annotation_default() -> Self {
        Self {
            color: [237, 68, 83],
            scale: 2.1,
            shaft_radius: 0.180,
            head_length: 1.0,
            head_radius: 2.5,
            penetrate_atom: true,
            add_atom_radius: true,
        }
    }

    fn magnetic_moment_default() -> Self {
        Self {
            color: [237, 68, 83],
            scale: 0.7,
            shaft_radius: 0.170,
            head_length: 0.85,
            head_radius: 2.3,
            penetrate_atom: true,
            add_atom_radius: true,
        }
    }
}

/// A single renderable vector definition and its attachment target.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct VectorLayer {
    pub(crate) enabled: bool,
    pub(crate) target: VectorTarget,
    pub(crate) kind: VectorKind,
    /// Components expressed in `coordinates`.
    pub(crate) components: [f32; 3],
    pub(crate) coordinates: VectorCoordinates,
    pub(crate) style: VectorStyle,
}

impl VectorLayer {
    pub(crate) fn for_site(atom: &SelectedAtom) -> Self {
        Self {
            enabled: true,
            target: VectorTarget::Site {
                site_id: atom.site_id,
            },
            kind: VectorKind::Axial,
            components: [0.0, 0.0, 1.0],
            coordinates: VectorCoordinates::Lattice,
            style: VectorStyle::annotation_default(),
        }
    }

    pub(crate) fn for_atom(atom: &SelectedAtom) -> Self {
        Self {
            enabled: true,
            target: VectorTarget::IndividualAtom {
                site_id: atom.site_id,
                fractional: atom.fractional,
            },
            kind: VectorKind::Axial,
            components: [0.0, 0.0, 1.0],
            coordinates: VectorCoordinates::Lattice,
            style: VectorStyle::annotation_default(),
        }
    }

    pub(crate) fn imported_site(
        site_id: SiteId,
        components: [f32; 3],
        color: [u8; 3],
        scale: f32,
        penetrate_atom: bool,
        add_atom_radius: bool,
    ) -> Self {
        Self {
            enabled: true,
            target: VectorTarget::Site { site_id },
            kind: VectorKind::Axial,
            components,
            coordinates: VectorCoordinates::Cartesian,
            style: VectorStyle {
                color,
                scale,
                // `VECTT`'s size is a VESTA display parameter, not a
                // calibrated world-space radius. Preserve Wyckoscope's
                // readable vector geometry instead of treating it as Å.
                shaft_radius: 0.180,
                head_length: 1.0,
                head_radius: 2.5,
                penetrate_atom,
                add_atom_radius,
            },
        }
    }

    pub(crate) fn magnetic_moment_site(site_id: SiteId, moment: Vec3) -> Self {
        Self {
            enabled: true,
            target: VectorTarget::Site { site_id },
            kind: VectorKind::Axial,
            components: moment.to_array(),
            coordinates: VectorCoordinates::Cartesian,
            style: VectorStyle::magnetic_moment_default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct VectorSettings {
    /// A presentation-only multiplier applied uniformly to every vector.
    pub(crate) display_scale: f32,
    pub(crate) layers: Vec<VectorLayer>,
}

impl Default for VectorSettings {
    fn default() -> Self {
        Self {
            display_scale: 1.0,
            layers: Vec::new(),
        }
    }
}

impl AtomDisplayMode {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::BallAndStick => "Ball & stick",
            Self::SpaceFilling => "Space filling",
            Self::Wireframe => "Wireframe",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum LightMode {
    Camera,
    World,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum KeyLightKind {
    Point,
    Spot,
    Directional,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct LightingSettings {
    pub(crate) ambient_enabled: bool,
    pub(crate) ambient_color: [u8; 3],
    pub(crate) ambient_brightness: f32,
    pub(crate) key_enabled: bool,
    pub(crate) key_kind: KeyLightKind,
    pub(crate) key_color: [u8; 3],
    pub(crate) key_mode: LightMode,
    pub(crate) key_position: [f32; 3],
    pub(crate) key_follow_distance: f32,
    pub(crate) key_follow_azimuth: f32,
    pub(crate) key_follow_elevation: f32,
    pub(crate) key_target_offset: [f32; 3],
    pub(crate) key_intensity: f32,
    pub(crate) key_directional_illuminance: f32,
    pub(crate) key_directional_rotation: [f32; 3],
    pub(crate) key_range: f32,
    pub(crate) key_radius: f32,
    pub(crate) key_shadows: bool,
    pub(crate) key_shadow_depth_bias: f32,
    pub(crate) key_shadow_normal_bias: f32,
    pub(crate) key_shadow_near_z: f32,
    pub(crate) spot_inner_angle: f32,
    pub(crate) spot_outer_angle: f32,
    pub(crate) fill_enabled: bool,
    pub(crate) fill_color: [u8; 3],
    pub(crate) fill_illuminance: f32,
    pub(crate) fill_rotation: [f32; 3],
    pub(crate) fill_shadows: bool,
    pub(crate) fill_shadow_depth_bias: f32,
    pub(crate) fill_shadow_normal_bias: f32,
    pub(crate) axis_key_enabled: bool,
    pub(crate) axis_key_color: [u8; 3],
    pub(crate) axis_key_illuminance: f32,
    pub(crate) axis_key_rotation: [f32; 3],
    pub(crate) axis_fill_enabled: bool,
    pub(crate) axis_fill_color: [u8; 3],
    pub(crate) axis_fill_illuminance: f32,
    pub(crate) axis_fill_rotation: [f32; 3],
}

impl Default for LightingSettings {
    fn default() -> Self {
        Self {
            ambient_enabled: true,
            ambient_color: [255, 255, 255],
            ambient_brightness: 200.0,
            key_enabled: true,
            key_kind: KeyLightKind::Directional,
            key_color: [255, 255, 255],
            key_mode: LightMode::Camera,
            key_position: [8.0, 10.0, 12.0],
            key_follow_distance: 17.5,
            key_follow_azimuth: 45.0,
            key_follow_elevation: -89.0,
            key_target_offset: [0.0, 0.0, 0.0],
            key_intensity: 2_000_000.0,
            key_directional_illuminance: 4_000.0,
            key_directional_rotation: [-60.0, 45.0, 0.0],
            key_range: 100.0,
            key_radius: 1.5,
            key_shadows: false,
            key_shadow_depth_bias: 0.02,
            key_shadow_normal_bias: 0.55,
            key_shadow_near_z: 0.1,
            spot_inner_angle: 28.0,
            spot_outer_angle: 42.0,
            fill_enabled: false,
            fill_color: [133, 168, 255],
            fill_illuminance: 4_800.0,
            fill_rotation: [-45.84, 137.51, 0.0],
            fill_shadows: false,
            fill_shadow_depth_bias: 0.02,
            fill_shadow_normal_bias: 1.8,
            axis_key_enabled: true,
            axis_key_color: [255, 247, 235],
            axis_key_illuminance: 10_000.0,
            axis_key_rotation: [-37.24, -42.97, 0.0],
            axis_fill_enabled: true,
            axis_fill_color: [128, 173, 255],
            axis_fill_illuminance: 5_000.0,
            axis_fill_rotation: [25.78, 128.92, 0.0],
        }
    }
}

/// Whether an overlay label is rendered in black or in the colour associated
/// with the object it annotates (an axis or an element).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub(crate) enum OverlayLabelColor {
    #[default]
    Black,
    Matching,
}

impl OverlayLabelColor {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Black => "Black",
            Self::Matching => "Matching color",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum UnitCellDisplay {
    Hidden,
    Single,
    All,
}

impl UnitCellDisplay {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Hidden => "Do not show",
            Self::Single => "Single unit cell",
            Self::All => "All unit cells",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum CellLineStyle {
    Solid,
    Dotted,
    Dashed,
}

impl CellLineStyle {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Solid => "Solid lines",
            Self::Dotted => "Dotted lines",
            Self::Dashed => "Dashed lines",
        }
    }
}

#[derive(Resource, Clone, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct VisualSettings {
    pub(crate) supercell: [u32; 3],
    pub(crate) expand_symmetry: bool,
    pub(crate) show_boundary_atoms: bool,
    pub(crate) atom_scale: f32,
    pub(crate) atom_metallic: f32,
    pub(crate) atom_roughness: f32,
    pub(crate) atom_reflectance: f32,
    pub(crate) element_color_scheme: ElementColorScheme,
    pub(crate) show_thermal_ellipsoids: bool,
    pub(crate) thermal_ellipsoid_scale: f32,
    pub(crate) scene_objects: SceneObjects,
    pub(crate) bond_rules: Vec<BondRule>,
    pub(crate) atom_display_mode: AtomDisplayMode,
    pub(crate) polyhedron_show_edges: bool,
    pub(crate) polyhedron_edge_radius: f32,
    pub(crate) polyhedron_edge_color: [u8; 3],
    pub(crate) vectors: VectorSettings,
    pub(crate) unit_cell_display: UnitCellDisplay,
    pub(crate) unit_cell_line_style: CellLineStyle,
    pub(crate) unit_cell_line_width: f32,
    pub(crate) unit_cell_color: [u8; 3],
    pub(crate) show_outlines: bool,
    pub(crate) auto_rotate: bool,
    pub(crate) background: [u8; 3],
    pub(crate) axis_colors: [[u8; 3]; 3],
    pub(crate) axis_label_size: f32,
    pub(crate) axis_label_color: OverlayLabelColor,
    pub(crate) axis_label_bold: bool,
    pub(crate) show_element_legend: bool,
    pub(crate) element_legend_label_color: OverlayLabelColor,
    pub(crate) element_legend_label_bold: bool,
    pub(crate) element_legend_scale: f32,
    pub(crate) projection_blend: f32,
    pub(crate) hdr_tonemapping: bool,
    pub(crate) lighting: LightingSettings,
    pub(crate) exposure_ev: f32,
    pub(crate) export_size: [u32; 2],
}

impl Default for VisualSettings {
    fn default() -> Self {
        Self {
            supercell: [1, 1, 1],
            expand_symmetry: true,
            show_boundary_atoms: true,
            atom_scale: 0.8,
            atom_metallic: 0.52,
            atom_roughness: 0.29,
            atom_reflectance: 0.53,
            element_color_scheme: ElementColorScheme::default(),
            show_thermal_ellipsoids: false,
            thermal_ellipsoid_scale: 2.45,
            scene_objects: SceneObjects::default(),
            bond_rules: Vec::new(),
            atom_display_mode: AtomDisplayMode::BallAndStick,
            polyhedron_show_edges: true,
            polyhedron_edge_radius: 0.015,
            polyhedron_edge_color: [255, 255, 255],
            vectors: VectorSettings::default(),
            unit_cell_display: UnitCellDisplay::All,
            unit_cell_line_style: CellLineStyle::Solid,
            unit_cell_line_width: 0.6,
            unit_cell_color: [0, 0, 0],
            show_outlines: false,
            auto_rotate: false,
            background: [255, 255, 255],
            axis_colors: [[235, 31, 31], [31, 184, 51], [31, 82, 245]],
            axis_label_size: 20.0,
            axis_label_color: OverlayLabelColor::Matching,
            axis_label_bold: true,
            show_element_legend: true,
            element_legend_label_color: OverlayLabelColor::Black,
            element_legend_label_bold: false,
            element_legend_scale: 1.0,
            projection_blend: 1.0,
            hdr_tonemapping: false,
            lighting: LightingSettings::default(),
            exposure_ev: 9.0,
            export_size: [3840, 2160],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct ElementAppearance {
    pub(crate) color: [u8; 3],
    pub(crate) radius: f32,
    pub(crate) visible: bool,
}

impl Default for ElementAppearance {
    fn default() -> Self {
        Self {
            color: [160, 160, 160],
            radius: 1.0,
            visible: true,
        }
    }
}

#[derive(Resource, Default, Clone, Serialize, Deserialize)]
pub(crate) struct ElementOverrides(pub(crate) HashMap<String, ElementAppearance>);

#[derive(Resource)]
pub(crate) struct SceneDirty {
    pub(crate) rebuild: bool,
    pub(crate) reset_camera: bool,
}

impl SceneDirty {
    pub(crate) fn mark(&mut self, reset_camera: bool) {
        self.rebuild = true;
        self.reset_camera |= reset_camera;
    }
}

#[derive(Resource, Default)]
pub(crate) struct SceneStatus {
    pub(crate) warning: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct SelectedAtom {
    /// Identity of the asymmetric crystallographic site that generated this atom.
    pub(crate) site_id: SiteId,
    pub(crate) element: String,
    pub(crate) label: String,
    pub(crate) position: Vec3,
    pub(crate) fractional: Vec3,
    pub(crate) coordination: usize,
}

#[derive(Resource, Default, Clone)]
pub(crate) struct SelectionState {
    pub(crate) atoms: Vec<SelectedAtom>,
}

#[derive(Resource, Default)]
pub(crate) struct SelectionPointer {
    pub(crate) press_position: Option<Vec2>,
}

#[derive(Resource)]
pub(crate) struct InspectorLayout {
    pub(crate) width: f32,
}

impl Default for InspectorLayout {
    fn default() -> Self {
        Self { width: 340.0 }
    }
}

#[derive(Resource, Default)]
pub(crate) struct AsyncDialogs {
    pub(crate) open_receiver: Option<Mutex<mpsc::Receiver<Option<PathBuf>>>>,
    pub(crate) save_receiver: Option<Mutex<mpsc::Receiver<Option<PathBuf>>>>,
    pub(crate) project_open_receiver: Option<Mutex<mpsc::Receiver<Option<PathBuf>>>>,
    pub(crate) project_save_receiver: Option<Mutex<mpsc::Receiver<Option<PathBuf>>>>,
    pub(crate) save_size: [u32; 2],
}

#[derive(Resource, Default)]
pub(crate) struct ExportState {
    pub(crate) request: Option<ExportRequest>,
    pub(crate) stage: Option<ExportStage>,
    pub(crate) capturing: bool,
    pub(crate) message: Option<String>,
}

#[derive(Resource, Default)]
pub(crate) struct LegendPreviews {
    pub(crate) entries: HashMap<String, LegendPreview>,
    pub(crate) fingerprint: String,
}

pub(crate) struct LegendPreview {
    pub(crate) texture_id: egui::TextureId,
    pub(crate) _image: Handle<Image>,
}

pub(crate) struct ExportRequest {
    pub(crate) path: PathBuf,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

pub(crate) struct ExportStage {
    pub(crate) image: Handle<Image>,
    pub(crate) camera: Entity,
    pub(crate) background_camera: Entity,
    pub(crate) path: PathBuf,
    pub(crate) frames_remaining: u8,
}

#[derive(Component)]
pub(crate) struct CrystalVisual;
#[derive(Component)]
pub(crate) struct LegendPreviewVisual;
#[derive(Component)]
pub(crate) struct ExportCamera;
#[derive(Component)]
pub(crate) struct BackgroundCamera;
#[derive(Component)]
pub(crate) enum LightRole {
    KeyPoint,
    KeySpot,
    KeyDirectional,
    Fill,
    AxisKey,
    AxisFill,
}
#[derive(Component)]
pub(crate) struct InterfaceCamera;
#[derive(Component)]
pub(crate) struct OrientationAxisCamera;
#[derive(Component)]
pub(crate) struct OrientationAxisRoot(pub(crate) usize);

#[derive(Resource)]
pub(crate) struct OrientationAxisMaterials(pub(crate) [Handle<StandardMaterial>; 3]);

#[derive(Component)]
pub(crate) struct OrbitCamera {
    pub(crate) target: Vec3,
    pub(crate) radius: f32,
    pub(crate) rotation: Quat,
    pub(crate) arcball_anchor: Option<Vec3>,
}

impl OrbitCamera {
    pub(crate) fn reset(&mut self) {
        self.target = Vec3::ZERO;
        self.rotation = Quat::from_euler(EulerRot::YXZ, 2.2, -0.32, 0.0);
        self.arcball_anchor = None;
    }

    pub(crate) fn look_from(&mut self, direction: Vec3) {
        let direction = direction.normalize_or_zero();
        if direction.length_squared() < 0.5 {
            return;
        }
        self.target = Vec3::ZERO;
        self.rotation = Transform::from_translation(direction)
            .looking_at(Vec3::ZERO, Vec3::Y)
            .rotation;
        self.arcball_anchor = None;
    }
}

#[derive(Clone)]
pub(crate) struct RenderAtom {
    /// Identity of the asymmetric crystallographic site that generated this atom.
    pub(crate) site_id: SiteId,
    pub(crate) position: Vec3,
    pub(crate) element: String,
    pub(crate) label: String,
    pub(crate) fractional: Vec3,
    pub(crate) symmetry_rotation: Mat3,
    pub(crate) symmetry_time_reversal: f32,
    pub(crate) occupancy: f32,
    pub(crate) anisotropic_u: Option<[[f32; 3]; 3]>,
}

impl SelectedAtom {
    pub(crate) fn matches(&self, atom: &RenderAtom) -> bool {
        self.site_id == atom.site_id && self.fractional.distance_squared(atom.fractional) < 1.0e-8
    }
}
