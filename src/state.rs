use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Mutex, mpsc},
};

use bevy::prelude::*;
use bevy_egui::egui;
use wyckoscope::model::CrystalStructure;

#[derive(Resource)]
pub(crate) struct AppState {
    pub(crate) structure: CrystalStructure,
    pub(crate) file_name: String,
    pub(crate) error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InspectorTab {
    Structure,
    View,
    Elements,
    Export,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AtomDisplayMode {
    BallAndStick,
    SpaceFilling,
    Wireframe,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

#[derive(Debug, Clone)]
pub(crate) struct BondRule {
    pub(crate) enabled: bool,
    pub(crate) center: String,
    pub(crate) neighbor: String,
    pub(crate) min_length: f32,
    pub(crate) max_length: f32,
    pub(crate) show_polyhedra: bool,
}

impl Default for BondRule {
    fn default() -> Self {
        Self {
            enabled: true,
            center: String::new(),
            neighbor: String::new(),
            min_length: 0.0,
            max_length: 2.5,
            show_polyhedra: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VectorAttachment {
    Site,
    IndividualAtom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VectorCoordinates {
    Lattice,
    Cartesian,
}

impl VectorAttachment {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Site => "Site",
            Self::IndividualAtom => "Atom",
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct AtomVector {
    pub(crate) attachment: VectorAttachment,
    pub(crate) label: String,
    pub(crate) element: String,
    pub(crate) position: Option<Vec3>,
    /// Components in lattice-vector notation, (u, v, w).
    pub(crate) components: [f32; 3],
    pub(crate) coordinates: VectorCoordinates,
    pub(crate) color: [u8; 3],
    pub(crate) scale: f32,
    pub(crate) penetrate_atom: bool,
    pub(crate) add_atom_radius: bool,
}

impl AtomVector {
    pub(crate) fn for_site(atom: &SelectedAtom) -> Self {
        Self {
            attachment: VectorAttachment::Site,
            label: atom.label.clone(),
            element: atom.element.clone(),
            position: None,
            components: [0.0, 0.0, 1.0],
            coordinates: VectorCoordinates::Lattice,
            color: [237, 68, 83],
            scale: 0.35,
            penetrate_atom: false,
            add_atom_radius: true,
        }
    }

    pub(crate) fn for_atom(atom: &SelectedAtom) -> Self {
        Self {
            attachment: VectorAttachment::IndividualAtom,
            label: atom.label.clone(),
            element: atom.element.clone(),
            position: Some(atom.position),
            components: [0.0, 0.0, 1.0],
            coordinates: VectorCoordinates::Lattice,
            color: [237, 68, 83],
            scale: 0.35,
            penetrate_atom: false,
            add_atom_radius: true,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LightMode {
    Camera,
    World,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KeyLightKind {
    Point,
    Spot,
}

#[derive(Clone)]
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
            ambient_color: [209, 222, 250],
            ambient_brightness: 520.0,
            key_enabled: true,
            key_kind: KeyLightKind::Point,
            key_color: [255, 240, 219],
            key_mode: LightMode::Camera,
            key_position: [8.0, 10.0, 12.0],
            key_follow_distance: 17.5,
            key_follow_azimuth: 45.0,
            key_follow_elevation: -89.0,
            key_target_offset: [0.0, 0.0, 0.0],
            key_intensity: 2_000_000.0,
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
            axis_key_illuminance: 16_000.0,
            axis_key_rotation: [-37.24, -42.97, 0.0],
            axis_fill_enabled: true,
            axis_fill_color: [128, 173, 255],
            axis_fill_illuminance: 5_000.0,
            axis_fill_rotation: [25.78, 128.92, 0.0],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AxisLabelFont {
    SansSerif,
    Monospace,
}

impl AxisLabelFont {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::SansSerif => "Sans serif",
            Self::Monospace => "Monospace",
        }
    }

    pub(crate) fn family(self) -> egui::FontFamily {
        match self {
            Self::SansSerif => egui::FontFamily::Proportional,
            Self::Monospace => egui::FontFamily::Monospace,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

#[derive(Resource)]
pub(crate) struct VisualSettings {
    pub(crate) tab: InspectorTab,
    pub(crate) supercell: [u32; 3],
    pub(crate) expand_symmetry: bool,
    /// Draw periodic copies of sites that lie on the lower boundary at the
    /// corresponding upper boundary of the displayed supercell.
    pub(crate) show_boundary_atoms: bool,
    pub(crate) atom_scale: f32,
    pub(crate) bond_radius: f32,
    pub(crate) bond_cutoff: f32,
    pub(crate) show_bonds: bool,
    pub(crate) bond_match_mode: BondMatchMode,
    pub(crate) bond_rules: Vec<BondRule>,
    pub(crate) atom_display_mode: AtomDisplayMode,
    pub(crate) show_polyhedra: bool,
    pub(crate) polyhedron_color: [u8; 3],
    pub(crate) polyhedron_opacity: f32,
    pub(crate) atom_vectors: Vec<AtomVector>,
    pub(crate) show_imported_moments: bool,
    pub(crate) imported_moment_color: [u8; 3],
    pub(crate) imported_moment_scale: f32,
    pub(crate) unit_cell_display: UnitCellDisplay,
    pub(crate) unit_cell_line_style: CellLineStyle,
    pub(crate) unit_cell_line_width: f32,
    pub(crate) unit_cell_color: [u8; 3],
    pub(crate) show_outlines: bool,
    pub(crate) show_floor: bool,
    pub(crate) auto_rotate: bool,
    pub(crate) background: [u8; 3],
    pub(crate) ground_color: [u8; 3],
    pub(crate) axis_colors: [[u8; 3]; 3],
    pub(crate) axis_label_size: f32,
    pub(crate) axis_label_font: AxisLabelFont,
    pub(crate) projection_blend: f32,
    pub(crate) hdr_tonemapping: bool,
    pub(crate) lighting: LightingSettings,
    pub(crate) exposure_ev: f32,
    pub(crate) export_size: [u32; 2],
}

impl Default for VisualSettings {
    fn default() -> Self {
        Self {
            tab: InspectorTab::Structure,
            supercell: [1, 1, 1],
            expand_symmetry: true,
            show_boundary_atoms: true,
            atom_scale: 1.0,
            bond_radius: 0.095,
            bond_cutoff: 2.35,
            show_bonds: false,
            bond_match_mode: BondMatchMode::Element,
            bond_rules: Vec::new(),
            atom_display_mode: AtomDisplayMode::BallAndStick,
            show_polyhedra: false,
            polyhedron_color: [74, 164, 255],
            polyhedron_opacity: 0.30,
            atom_vectors: Vec::new(),
            show_imported_moments: true,
            imported_moment_color: [237, 68, 83],
            imported_moment_scale: 0.35,
            unit_cell_display: UnitCellDisplay::All,
            unit_cell_line_style: CellLineStyle::Solid,
            unit_cell_line_width: 0.6,
            unit_cell_color: [0, 0, 0],
            show_outlines: true,
            show_floor: false,
            auto_rotate: false,
            background: [255, 255, 255],
            ground_color: [205, 211, 222],
            axis_colors: [[235, 31, 31], [31, 184, 51], [31, 82, 245]],
            axis_label_size: 20.0,
            axis_label_font: AxisLabelFont::SansSerif,
            projection_blend: 0.0,
            hdr_tonemapping: true,
            lighting: LightingSettings::default(),
            exposure_ev: 7.5,
            export_size: [3840, 2160],
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ElementAppearance {
    pub(crate) color: [u8; 3],
    pub(crate) radius: f32,
    pub(crate) visible: bool,
}

#[derive(Resource, Default)]
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
    pub(crate) element: String,
    pub(crate) label: String,
    pub(crate) position: Vec3,
    pub(crate) fractional: Vec3,
    pub(crate) coordination: usize,
}

#[derive(Resource, Default)]
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
    pub(crate) save_size: [u32; 2],
}

#[derive(Resource, Default)]
pub(crate) struct ExportState {
    pub(crate) request: Option<ExportRequest>,
    pub(crate) stage: Option<ExportStage>,
    pub(crate) capturing: bool,
    pub(crate) message: Option<String>,
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
pub(crate) struct ExportCamera;
#[derive(Component)]
pub(crate) struct BackgroundCamera;
#[derive(Component)]
pub(crate) enum LightRole {
    KeyPoint,
    KeySpot,
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
        self.rotation = Quat::from_euler(EulerRot::YXZ, -0.68, -0.42, 0.0);
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
    pub(crate) position: Vec3,
    pub(crate) element: String,
    pub(crate) label: String,
    pub(crate) fractional: Vec3,
    pub(crate) symmetry_rotation: Mat3,
    pub(crate) magnetic_moment: Option<Vec3>,
    pub(crate) occupancy: f32,
}

impl SelectedAtom {
    pub(crate) fn matches(&self, atom: &RenderAtom) -> bool {
        self.element == atom.element
            && self.label == atom.label
            && self.position.distance_squared(atom.position) < 1.0e-8
    }
}
