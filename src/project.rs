use std::path::Path;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use wyckoscope::model::{
    AtomSite, CrystalStructure, ImportedBondRule, ImportedVector, ImportedVisuals,
    SymmetryMetadata, UnitCell,
};

use crate::{
    camera::update_camera_transform,
    state::{AppState, ElementOverrides, OrbitCamera, SceneDirty, SelectionState, VisualSettings},
};

pub(crate) const PROJECT_FORMAT_VERSION: u32 = 1;
const HISTORY_LIMIT: usize = 100;
const HISTORY_COALESCE_SECONDS: f64 = 0.30;

/// Complete user-editable project state. Runtime UI and render entities are
/// deliberately excluded, so the same value can serve as an undo snapshot and
/// as the on-disk project representation.
#[derive(Clone)]
pub(crate) struct ProjectDocument {
    structure: CrystalStructure,
    file_name: String,
    visual_settings: VisualSettings,
    element_overrides: ElementOverrides,
    camera: CameraSnapshot,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
struct CameraSnapshot {
    target: [f32; 3],
    radius: f32,
    rotation: [f32; 4],
}

#[derive(Resource, Default)]
pub(crate) struct ProjectHistory {
    undo: Vec<ProjectDocument>,
    redo: Vec<ProjectDocument>,
    current: Option<ProjectDocument>,
    fingerprint: Option<String>,
    pending: Option<ProjectDocument>,
    pending_fingerprint: Option<String>,
    pending_since: f64,
}

#[derive(Resource, Default)]
pub(crate) struct UndoRedoRequests {
    pub(crate) undo: bool,
    pub(crate) redo: bool,
}

pub(crate) fn initialize_history(
    state: Res<AppState>,
    settings: Res<VisualSettings>,
    overrides: Res<ElementOverrides>,
    camera: Single<&OrbitCamera>,
    mut history: ResMut<ProjectHistory>,
) {
    history.reset(capture_document(&state, &settings, &overrides, &camera));
}

pub(crate) fn track_history(
    state: Res<AppState>,
    settings: Res<VisualSettings>,
    overrides: Res<ElementOverrides>,
    camera: Single<&OrbitCamera>,
    time: Res<Time>,
    mut history: ResMut<ProjectHistory>,
) {
    let document = capture_document(&state, &settings, &overrides, &camera);
    let fingerprint = document_fingerprint(&document);
    if history.current.is_none() {
        history.current = Some(document);
        history.fingerprint = Some(fingerprint);
        return;
    }
    if history.fingerprint.as_deref() == Some(fingerprint.as_str()) {
        history.pending = None;
        history.pending_fingerprint = None;
        return;
    }
    if history.pending_fingerprint.as_deref() != Some(fingerprint.as_str()) {
        history.pending = Some(document);
        history.pending_fingerprint = Some(fingerprint);
        history.pending_since = time.elapsed_secs_f64();
        return;
    }
    if time.elapsed_secs_f64() - history.pending_since >= HISTORY_COALESCE_SECONDS {
        history.commit_pending();
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_history_requests(
    mut requests: ResMut<UndoRedoRequests>,
    mut state: ResMut<AppState>,
    mut settings: ResMut<VisualSettings>,
    mut overrides: ResMut<ElementOverrides>,
    mut selection: ResMut<SelectionState>,
    mut dirty: ResMut<SceneDirty>,
    mut camera: Single<(&mut OrbitCamera, &mut Transform), With<OrbitCamera>>,
    mut history: ResMut<ProjectHistory>,
) {
    let undo = std::mem::take(&mut requests.undo);
    let redo = std::mem::take(&mut requests.redo);
    if undo == redo {
        return;
    }
    history.commit_pending();
    let next = if undo {
        let Some(previous) = history.undo.pop() else {
            return;
        };
        if let Some(current) = history.current.take() {
            history.redo.push(current);
        }
        previous
    } else {
        let Some(next) = history.redo.pop() else {
            return;
        };
        if let Some(current) = history.current.take() {
            history.undo.push(current);
        }
        next
    };
    let (orbit, transform) = &mut *camera;
    restore_document(
        next.clone(),
        &mut state,
        &mut settings,
        &mut overrides,
        &mut selection,
        &mut dirty,
        orbit,
        transform,
    );
    history.fingerprint = Some(document_fingerprint(&next));
    history.current = Some(next);
}

impl ProjectHistory {
    pub(crate) fn reset(&mut self, document: ProjectDocument) {
        self.fingerprint = Some(document_fingerprint(&document));
        self.current = Some(document);
        self.undo.clear();
        self.redo.clear();
        self.pending = None;
        self.pending_fingerprint = None;
    }

    pub(crate) fn clear(&mut self) {
        self.current = None;
        self.fingerprint = None;
        self.undo.clear();
        self.redo.clear();
        self.pending = None;
        self.pending_fingerprint = None;
    }

    pub(crate) fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub(crate) fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    fn commit_pending(&mut self) {
        let Some(next) = self.pending.take() else {
            return;
        };
        let next_fingerprint = self
            .pending_fingerprint
            .take()
            .unwrap_or_else(|| document_fingerprint(&next));
        if let Some(current) = self.current.replace(next) {
            self.undo.push(current);
            if self.undo.len() > HISTORY_LIMIT {
                self.undo.remove(0);
            }
        }
        self.redo.clear();
        self.fingerprint = Some(next_fingerprint);
    }
}

pub(crate) fn capture_document(
    state: &AppState,
    settings: &VisualSettings,
    overrides: &ElementOverrides,
    camera: &OrbitCamera,
) -> ProjectDocument {
    ProjectDocument {
        structure: state.structure.clone(),
        file_name: state.file_name.clone(),
        visual_settings: settings.clone(),
        element_overrides: overrides.clone(),
        camera: CameraSnapshot {
            target: camera.target.to_array(),
            radius: camera.radius,
            rotation: camera.rotation.to_array(),
        },
    }
}

pub(crate) fn write_project(path: &Path, document: ProjectDocument) -> Result<(), String> {
    let file = ProjectFile::from(document);
    let bytes = serde_json::to_vec_pretty(&file).map_err(|error| error.to_string())?;
    std::fs::write(path, bytes).map_err(|error| error.to_string())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn read_project(
    path: &Path,
    state: &mut AppState,
    settings: &mut VisualSettings,
    overrides: &mut ElementOverrides,
    selection: &mut SelectionState,
    dirty: &mut SceneDirty,
    camera: &mut OrbitCamera,
    camera_transform: &mut Transform,
    history: &mut ProjectHistory,
) -> Result<(), String> {
    let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
    let file: ProjectFile = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    if file.format_version != PROJECT_FORMAT_VERSION {
        return Err(format!(
            "Unsupported Wyckoscope project format {}.",
            file.format_version
        ));
    }
    let document = file.try_into_document()?;
    restore_document(
        document.clone(),
        state,
        settings,
        overrides,
        selection,
        dirty,
        camera,
        camera_transform,
    );
    history.reset(document);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn restore_document(
    document: ProjectDocument,
    state: &mut AppState,
    settings: &mut VisualSettings,
    overrides: &mut ElementOverrides,
    selection: &mut SelectionState,
    dirty: &mut SceneDirty,
    camera: &mut OrbitCamera,
    camera_transform: &mut Transform,
) {
    state.structure = document.structure;
    state.file_name = document.file_name;
    state.error = None;
    *settings = document.visual_settings;
    *overrides = document.element_overrides;
    camera.target = Vec3::from_array(document.camera.target);
    camera.radius = document.camera.radius.max(0.01);
    camera.rotation = Quat::from_array(document.camera.rotation).normalize();
    camera.arcball_anchor = None;
    update_camera_transform(camera, camera_transform);
    selection.atoms.clear();
    // The document already contains the intended camera. Rebuilding with a
    // camera reset would overwrite it on the following system, which the
    // history tracker would then interpret as a fresh edit and clear redo.
    dirty.mark(false);
}

fn document_fingerprint(document: &ProjectDocument) -> String {
    serde_json::to_string(&ProjectFile::from(document.clone()))
        .expect("project document must be serializable")
}

#[derive(Serialize, Deserialize)]
struct ProjectFile {
    format_version: u32,
    structure: StructureFile,
    file_name: String,
    visual_settings: VisualSettings,
    element_overrides: ElementOverrides,
    camera: CameraSnapshot,
}

impl From<ProjectDocument> for ProjectFile {
    fn from(document: ProjectDocument) -> Self {
        Self {
            format_version: PROJECT_FORMAT_VERSION,
            structure: StructureFile::from(document.structure),
            file_name: document.file_name,
            visual_settings: document.visual_settings,
            element_overrides: document.element_overrides,
            camera: document.camera,
        }
    }
}

impl ProjectFile {
    fn try_into_document(self) -> Result<ProjectDocument, String> {
        Ok(ProjectDocument {
            structure: self.structure.try_into_structure()?,
            file_name: self.file_name,
            visual_settings: self.visual_settings,
            element_overrides: self.element_overrides,
            camera: self.camera,
        })
    }
}

#[derive(Serialize, Deserialize)]
struct StructureFile {
    title: String,
    cell: CellFile,
    asymmetric_atoms: Vec<AtomFile>,
    atoms: Vec<AtomFile>,
    space_group: Option<String>,
    #[serde(default)]
    symmetry_metadata: SymmetryMetadataFile,
    symmetry_operation_count: usize,
    source_format: wyckoscope::model::SourceFormat,
    warnings: Vec<String>,
    imported_visuals: ImportedVisualsFile,
}

#[derive(Serialize, Deserialize)]
struct CellFile {
    a: f32,
    b: f32,
    c: f32,
    alpha: f32,
    beta: f32,
    gamma: f32,
}

#[derive(Serialize, Deserialize)]
struct AtomFile {
    site_id: u64,
    element: String,
    label: String,
    fractional: [f32; 3],
    #[serde(default)]
    fractional_as_written: Option<[f32; 3]>,
    occupancy: f32,
    #[serde(default)]
    u_iso: Option<f32>,
    symmetry_rotation: [f32; 9],
    symmetry_time_reversal: f32,
    magnetic_moment: Option<[f32; 3]>,
}

#[derive(Default, Serialize, Deserialize)]
struct SymmetryMetadataFile {
    lattice_centering: Option<String>,
    space_group_number: Option<u16>,
    setting: Option<String>,
    parent_space_group_name: Option<String>,
    parent_space_group_number: Option<u16>,
    magnetic_space_group_name_bns: Option<String>,
    magnetic_space_group_number_bns: Option<String>,
    transformation_to_standard: Option<String>,
    transformation_from_standard: Option<String>,
}

#[derive(Default, Serialize, Deserialize)]
struct ImportedVisualsFile {
    background: Option<[u8; 3]>,
    bond_rules: Vec<ImportedBondRuleFile>,
    vectors: Vec<ImportedVectorFile>,
}

#[derive(Serialize, Deserialize)]
struct ImportedBondRuleFile {
    center: String,
    neighbor: String,
    min_length: f32,
    max_length: f32,
    show_polyhedra: bool,
}

#[derive(Serialize, Deserialize)]
struct ImportedVectorFile {
    label: String,
    components: [f32; 3],
    color: [u8; 3],
    scale: f32,
    penetrate_atom: bool,
    add_atom_radius: bool,
}

impl From<CrystalStructure> for StructureFile {
    fn from(structure: CrystalStructure) -> Self {
        Self {
            title: structure.title,
            cell: CellFile {
                a: structure.cell.a,
                b: structure.cell.b,
                c: structure.cell.c,
                alpha: structure.cell.alpha,
                beta: structure.cell.beta,
                gamma: structure.cell.gamma,
            },
            asymmetric_atoms: structure
                .asymmetric_atoms
                .into_iter()
                .map(AtomFile::from)
                .collect(),
            atoms: structure.atoms.into_iter().map(AtomFile::from).collect(),
            space_group: structure.space_group,
            symmetry_metadata: SymmetryMetadataFile::from(structure.symmetry_metadata),
            symmetry_operation_count: structure.symmetry_operation_count,
            source_format: structure.source_format,
            warnings: structure.warnings,
            imported_visuals: ImportedVisualsFile::from(structure.imported_visuals),
        }
    }
}

impl StructureFile {
    fn try_into_structure(self) -> Result<CrystalStructure, String> {
        let cell = UnitCell::new(
            self.cell.a,
            self.cell.b,
            self.cell.c,
            self.cell.alpha,
            self.cell.beta,
            self.cell.gamma,
        );
        if cell.volume() <= 1.0e-6 {
            return Err("Project contains an invalid unit cell.".to_string());
        }
        Ok(CrystalStructure {
            title: self.title,
            cell,
            asymmetric_atoms: self
                .asymmetric_atoms
                .into_iter()
                .map(AtomFile::try_into_atom)
                .collect::<Result<_, _>>()?,
            atoms: self
                .atoms
                .into_iter()
                .map(AtomFile::try_into_atom)
                .collect::<Result<_, _>>()?,
            space_group: self.space_group,
            symmetry_metadata: self.symmetry_metadata.into(),
            symmetry_operation_count: self.symmetry_operation_count,
            source_format: self.source_format,
            warnings: self.warnings,
            imported_visuals: self.imported_visuals.into(),
        })
    }
}

impl From<AtomSite> for AtomFile {
    fn from(atom: AtomSite) -> Self {
        Self {
            site_id: atom.site_id.0,
            element: atom.element,
            label: atom.label,
            fractional: atom.fractional.to_array(),
            fractional_as_written: Some(atom.fractional_as_written.to_array()),
            occupancy: atom.occupancy,
            u_iso: atom.u_iso,
            symmetry_rotation: atom.symmetry_rotation.to_cols_array(),
            symmetry_time_reversal: atom.symmetry_time_reversal,
            magnetic_moment: atom.magnetic_moment.map(|moment| moment.to_array()),
        }
    }
}

impl AtomFile {
    fn try_into_atom(self) -> Result<AtomSite, String> {
        if !self.fractional.into_iter().all(f32::is_finite)
            || !self.symmetry_rotation.into_iter().all(f32::is_finite)
        {
            return Err("Project contains non-finite atom coordinates.".to_string());
        }
        Ok(AtomSite {
            site_id: wyckoscope::model::SiteId(self.site_id),
            element: self.element,
            label: self.label,
            fractional: Vec3::from_array(self.fractional),
            fractional_as_written: Vec3::from_array(
                self.fractional_as_written.unwrap_or(self.fractional),
            ),
            occupancy: self.occupancy,
            u_iso: self.u_iso,
            symmetry_rotation: Mat3::from_cols_array(&self.symmetry_rotation),
            symmetry_time_reversal: self.symmetry_time_reversal,
            magnetic_moment: self.magnetic_moment.map(Vec3::from_array),
        })
    }
}

impl From<SymmetryMetadata> for SymmetryMetadataFile {
    fn from(metadata: SymmetryMetadata) -> Self {
        Self {
            lattice_centering: metadata.lattice_centering,
            space_group_number: metadata.space_group_number,
            setting: metadata.setting,
            parent_space_group_name: metadata.parent_space_group_name,
            parent_space_group_number: metadata.parent_space_group_number,
            magnetic_space_group_name_bns: metadata.magnetic_space_group_name_bns,
            magnetic_space_group_number_bns: metadata.magnetic_space_group_number_bns,
            transformation_to_standard: metadata.transformation_to_standard,
            transformation_from_standard: metadata.transformation_from_standard,
        }
    }
}

impl From<SymmetryMetadataFile> for SymmetryMetadata {
    fn from(metadata: SymmetryMetadataFile) -> Self {
        Self {
            lattice_centering: metadata.lattice_centering,
            space_group_number: metadata.space_group_number,
            setting: metadata.setting,
            parent_space_group_name: metadata.parent_space_group_name,
            parent_space_group_number: metadata.parent_space_group_number,
            magnetic_space_group_name_bns: metadata.magnetic_space_group_name_bns,
            magnetic_space_group_number_bns: metadata.magnetic_space_group_number_bns,
            transformation_to_standard: metadata.transformation_to_standard,
            transformation_from_standard: metadata.transformation_from_standard,
        }
    }
}

impl From<ImportedVisuals> for ImportedVisualsFile {
    fn from(visuals: ImportedVisuals) -> Self {
        Self {
            background: visuals.background,
            bond_rules: visuals
                .bond_rules
                .into_iter()
                .map(|rule| ImportedBondRuleFile {
                    center: rule.center,
                    neighbor: rule.neighbor,
                    min_length: rule.min_length,
                    max_length: rule.max_length,
                    show_polyhedra: rule.show_polyhedra,
                })
                .collect(),
            vectors: visuals
                .vectors
                .into_iter()
                .map(|vector| ImportedVectorFile {
                    label: vector.label,
                    components: vector.components.to_array(),
                    color: vector.color,
                    scale: vector.scale,
                    penetrate_atom: vector.penetrate_atom,
                    add_atom_radius: vector.add_atom_radius,
                })
                .collect(),
        }
    }
}

impl From<ImportedVisualsFile> for ImportedVisuals {
    fn from(visuals: ImportedVisualsFile) -> Self {
        Self {
            background: visuals.background,
            bond_rules: visuals
                .bond_rules
                .into_iter()
                .map(|rule| ImportedBondRule {
                    center: rule.center,
                    neighbor: rule.neighbor,
                    min_length: rule.min_length,
                    max_length: rule.max_length,
                    show_polyhedra: rule.show_polyhedra,
                })
                .collect(),
            vectors: visuals
                .vectors
                .into_iter()
                .map(|vector| ImportedVector {
                    label: vector.label,
                    components: Vec3::from_array(vector.components),
                    color: vector.color,
                    scale: vector.scale,
                    penetrate_atom: vector.penetrate_atom,
                    add_atom_radius: vector.add_atom_radius,
                })
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use bevy::prelude::{EulerRot, Quat, Vec3};
    use wyckoscope::parser;

    use super::{CameraSnapshot, ProjectDocument, ProjectFile};
    use crate::state::{ElementOverrides, VectorLayer, VectorTarget, VisualSettings};

    #[test]
    fn project_file_round_trip_preserves_site_ids_and_vector_targets() {
        let structure = parser::parse_text(include_str!("../assets/samples/perovskite.cif"), "cif")
            .expect("bundled sample CIF is valid");
        let vector_site_id = structure.asymmetric_atoms[0].site_id;
        let mut visual_settings = VisualSettings::default();
        visual_settings
            .vectors
            .layers
            .push(VectorLayer::magnetic_moment_site(vector_site_id, Vec3::X));
        let document = ProjectDocument {
            structure,
            file_name: "perovskite.cif".to_string(),
            visual_settings,
            element_overrides: ElementOverrides::default(),
            camera: CameraSnapshot {
                target: [1.0, 2.0, 3.0],
                radius: 9.0,
                rotation: Quat::from_euler(EulerRot::YXZ, 0.2, -0.3, 0.0).to_array(),
            },
        };
        let encoded = serde_json::to_vec(&ProjectFile::from(document.clone()))
            .expect("project document serializes");
        let decoded: ProjectFile =
            serde_json::from_slice(&encoded).expect("project document deserializes");
        let restored = decoded
            .try_into_document()
            .expect("project document is valid");

        assert_eq!(
            restored.structure.asymmetric_atoms[0].site_id,
            document.structure.asymmetric_atoms[0].site_id
        );
        assert_eq!(restored.camera.target, Vec3::new(1.0, 2.0, 3.0).to_array());
        assert_eq!(restored.camera.radius, document.camera.radius);
        assert!(matches!(
            restored.visual_settings.vectors.layers[0].target,
            VectorTarget::Site { site_id } if site_id == vector_site_id
        ));
    }
}
