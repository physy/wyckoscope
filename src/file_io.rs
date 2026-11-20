use std::{
    path::Path,
    sync::{Mutex, mpsc},
};

use bevy::{prelude::*, tasks::IoTaskPool, window::FileDragAndDrop};
use wyckoscope::parser;

use crate::{
    project::{
        ProjectWorkspace, UndoRedoRequests, capture_document, document_from_structure,
        read_project_document, write_project,
    },
    state::*,
};

pub(crate) fn load_dropped_file(
    mut drops: MessageReader<FileDragAndDrop>,
    mut state: ResMut<AppState>,
    mut structure_selection: ResMut<StructureSelection>,
    mut workspace: ResMut<ProjectWorkspace>,
    camera: Single<&OrbitCamera>,
) {
    for event in drops.read() {
        if let FileDragAndDrop::DroppedFile { path_buf, .. } = event {
            load_file(
                path_buf,
                &mut state,
                &mut structure_selection,
                &mut workspace,
                &camera,
            );
        }
    }
}

pub(crate) fn keyboard_shortcuts(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut dialogs: ResMut<AsyncDialogs>,
    mut history_requests: ResMut<UndoRedoRequests>,
) {
    let command = keyboard.any_pressed([
        KeyCode::ControlLeft,
        KeyCode::ControlRight,
        KeyCode::SuperLeft,
        KeyCode::SuperRight,
    ]);
    if command && keyboard.just_pressed(KeyCode::KeyZ) {
        if keyboard.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]) {
            history_requests.redo = true;
        } else {
            history_requests.undo = true;
        }
    } else if command && keyboard.just_pressed(KeyCode::KeyY) {
        history_requests.redo = true;
    } else if command && keyboard.just_pressed(KeyCode::KeyS) {
        request_project_save_dialog(&mut dialogs);
    } else if keyboard.just_pressed(KeyCode::KeyO) {
        request_open_dialog(&mut dialogs);
    }
}

pub(crate) fn request_open_dialog(dialogs: &mut AsyncDialogs) {
    if dialogs.open_receiver.is_some() {
        return;
    }
    let (sender, receiver) = mpsc::channel();
    dialogs.open_receiver = Some(Mutex::new(receiver));
    IoTaskPool::get()
        .spawn(async move {
            let path = rfd::AsyncFileDialog::new()
                .set_title("Open a crystal structure")
                .add_filter("Crystal structures", &["cif", "mcif", "vesta"])
                .pick_file()
                .await
                .map(|handle| handle.path().to_path_buf());
            let _ = sender.send(path);
        })
        .detach();
}

pub(crate) fn request_save_dialog(dialogs: &mut AsyncDialogs, size: [u32; 2]) {
    if dialogs.save_receiver.is_some() {
        return;
    }
    dialogs.save_size = size;
    let (sender, receiver) = mpsc::channel();
    dialogs.save_receiver = Some(Mutex::new(receiver));
    IoTaskPool::get()
        .spawn(async move {
            let path = rfd::AsyncFileDialog::new()
                .set_title("Export high-resolution PNG")
                .set_file_name("crystal.png")
                .add_filter("PNG image", &["png"])
                .save_file()
                .await
                .map(|handle| handle.path().to_path_buf());
            let _ = sender.send(path);
        })
        .detach();
}

pub(crate) fn request_project_open_dialog(dialogs: &mut AsyncDialogs) {
    if dialogs.project_open_receiver.is_some() {
        return;
    }
    let (sender, receiver) = mpsc::channel();
    dialogs.project_open_receiver = Some(Mutex::new(receiver));
    IoTaskPool::get()
        .spawn(async move {
            let path = rfd::AsyncFileDialog::new()
                .set_title("Open Wyckoscope project")
                .add_filter("Wyckoscope project", &["wyckoscope"])
                .pick_file()
                .await
                .map(|handle| handle.path().to_path_buf());
            let _ = sender.send(path);
        })
        .detach();
}

pub(crate) fn request_project_save_dialog(dialogs: &mut AsyncDialogs) {
    if dialogs.project_save_receiver.is_some() {
        return;
    }
    let (sender, receiver) = mpsc::channel();
    dialogs.project_save_receiver = Some(Mutex::new(receiver));
    IoTaskPool::get()
        .spawn(async move {
            let path = rfd::AsyncFileDialog::new()
                .set_title("Save Wyckoscope project")
                .set_file_name("crystal.wyckoscope")
                .add_filter("Wyckoscope project", &["wyckoscope"])
                .save_file()
                .await
                .map(|handle| handle.path().to_path_buf());
            let _ = sender.send(path);
        })
        .detach();
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn poll_async_dialogs(
    mut dialogs: ResMut<AsyncDialogs>,
    mut state: ResMut<AppState>,
    settings: ResMut<VisualSettings>,
    mut structure_selection: ResMut<StructureSelection>,
    overrides: ResMut<ElementOverrides>,
    mut export: ResMut<ExportState>,
    camera: Single<(&mut OrbitCamera, &mut Transform), With<OrbitCamera>>,
    mut workspace: ResMut<ProjectWorkspace>,
) {
    let open = dialogs
        .open_receiver
        .as_ref()
        .and_then(|receiver| receiver.lock().ok().and_then(|value| value.try_recv().ok()));
    if let Some(path) = open {
        dialogs.open_receiver = None;
        if let Some(path) = path {
            load_file(
                &path,
                &mut state,
                &mut structure_selection,
                &mut workspace,
                &camera.0,
            );
        }
    }
    let save = dialogs
        .save_receiver
        .as_ref()
        .and_then(|receiver| receiver.lock().ok().and_then(|value| value.try_recv().ok()));
    if let Some(path) = save {
        dialogs.save_receiver = None;
        if let Some(mut path) = path {
            if path.extension().is_none() {
                path.set_extension("png");
            }
            export.request = Some(ExportRequest {
                path,
                width: dialogs.save_size[0],
                height: dialogs.save_size[1],
            });
            export.message = Some("Preparing high-resolution render…".to_string());
        }
    }
    let project_open = dialogs
        .project_open_receiver
        .as_ref()
        .and_then(|receiver| receiver.lock().ok().and_then(|value| value.try_recv().ok()));
    if let Some(path) = project_open {
        dialogs.project_open_receiver = None;
        if let Some(path) = path {
            match read_project_document(&path) {
                Ok(document) => workspace.request_open(document),
                Err(error) => state.error = Some(error),
            }
        }
    }
    let project_save = dialogs
        .project_save_receiver
        .as_ref()
        .and_then(|receiver| receiver.lock().ok().and_then(|value| value.try_recv().ok()));
    if let Some(path) = project_save {
        dialogs.project_save_receiver = None;
        if let Some(mut path) = path {
            if path.extension().is_none() {
                path.set_extension("wyckoscope");
            }
            let document = capture_document(&state, &settings, &overrides, &camera.0);
            match write_project(&path, document) {
                Ok(()) => state.error = None,
                Err(error) => state.error = Some(format!("Could not save project: {error}")),
            }
        }
    }
}

fn load_file(
    path: &Path,
    state: &mut AppState,
    structure_selection: &mut StructureSelection,
    workspace: &mut ProjectWorkspace,
    camera: &OrbitCamera,
) {
    structure_selection.candidates.clear();
    structure_selection.selected = None;
    if path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("wyckoscope"))
    {
        match read_project_document(path) {
            Ok(document) => {
                workspace.request_open(document);
                state.error = None;
            }
            Err(error) => state.error = Some(error),
        }
        return;
    }
    match parser::parse_path_candidates(path) {
        Ok(mut candidates) if candidates.len() == 1 => {
            let candidate = candidates.remove(0);
            apply_structure(
                candidate.structure,
                path.file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or("Structure"),
                &candidate.data_name,
                workspace,
                camera,
            );
            state.error = None;
        }
        Ok(candidates) => {
            structure_selection.file_name = path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("Structure")
                .to_string();
            structure_selection.candidates = candidates;
            state.error = None;
        }
        Err(error) => {
            state.error = Some(error.to_string());
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_selected_structure(
    mut structure_selection: ResMut<StructureSelection>,
    mut workspace: ResMut<ProjectWorkspace>,
    camera: Single<&OrbitCamera>,
) {
    let Some(index) = structure_selection.selected.take() else {
        return;
    };
    let Some(candidate) = structure_selection.candidates.get(index).cloned() else {
        structure_selection.candidates.clear();
        return;
    };
    apply_structure(
        candidate.structure,
        &structure_selection.file_name,
        &candidate.data_name,
        &mut workspace,
        &camera,
    );
    structure_selection.candidates.clear();
}

fn apply_structure(
    mut structure: wyckoscope::model::CrystalStructure,
    file_name: &str,
    data_name: &str,
    workspace: &mut ProjectWorkspace,
    camera: &OrbitCamera,
) {
    if matches!(
        structure.title.trim(),
        "" | "." | "?" | "Untitled structure"
    ) {
        structure.title = Path::new(file_name)
            .file_stem()
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .unwrap_or("Untitled structure")
            .to_string();
    }
    let mut settings = VisualSettings::default();
    apply_imported_visuals(&mut settings, &structure);
    let tab_name = if data_name.is_empty() {
        file_name.to_string()
    } else {
        format!("{file_name} · {data_name}")
    };
    workspace.request_open(document_from_structure(
        structure, tab_name, settings, camera,
    ));
}

fn apply_imported_visuals(
    settings: &mut VisualSettings,
    structure: &wyckoscope::model::CrystalStructure,
) {
    let visuals = &structure.imported_visuals;
    if let Some(background) = visuals.background {
        settings.background = background;
    }
    // Every source format produces ordinary, editable vectors. The source
    // format is retained by the structure, not by a separate vector UI path.
    settings.vectors.layers.clear();
    settings
        .vectors
        .layers
        .extend(structure.asymmetric_atoms.iter().filter_map(|atom| {
            atom.magnetic_moment
                .map(|moment| VectorLayer::magnetic_moment_site(atom.site_id, moment))
        }));
    if structure.source_format == wyckoscope::model::SourceFormat::Vesta {
        settings.bond_rules = visuals
            .bond_rules
            .iter()
            .map(|rule| BondRule {
                enabled: true,
                match_mode: if rule.match_by_label {
                    BondMatchMode::SiteLabel
                } else {
                    BondMatchMode::Element
                },
                search_mode: match rule.search_mode {
                    wyckoscope::model::ImportedBondSearchMode::A2BondedToA1 => {
                        BondSearchMode::A2BondedToA1
                    }
                    wyckoscope::model::ImportedBondSearchMode::AtomsBondedToA1 => {
                        BondSearchMode::AtomsBondedToA1
                    }
                    wyckoscope::model::ImportedBondSearchMode::Molecules => BondSearchMode::Molecules,
                },
                boundary_mode: match rule.boundary_mode {
                    wyckoscope::model::ImportedBondBoundaryMode::DoNotSearchBeyond => {
                        BondBoundaryMode::DoNotSearchBeyond
                    }
                    wyckoscope::model::ImportedBondBoundaryMode::SearchAdditionalForA1 => {
                        BondBoundaryMode::SearchAdditionalForA1
                    }
                    wyckoscope::model::ImportedBondBoundaryMode::SearchRecursivelyIfEitherVisible => {
                        BondBoundaryMode::SearchRecursivelyIfEitherVisible
                    }
                },
                center: rule.center.clone(),
                neighbor: rule.neighbor.clone(),
                min_length: rule.min_length,
                max_length: rule.max_length,
                bond_style: match rule.style {
                    wyckoscope::model::ImportedBondStyle::UnicolorCylinder => {
                        BondStyle::UnicolorCylinder
                    }
                    wyckoscope::model::ImportedBondStyle::BicolorCylinder => {
                        BondStyle::BicolorCylinder
                    }
                    wyckoscope::model::ImportedBondStyle::ColorLine => BondStyle::ColorLine,
                    wyckoscope::model::ImportedBondStyle::GradientLine => BondStyle::GradientLine,
                    wyckoscope::model::ImportedBondStyle::DottedLine => BondStyle::DottedLine,
                    wyckoscope::model::ImportedBondStyle::DashedLine => BondStyle::DashedLine,
                },
                bond_radius: rule.radius,
                bond_pixel_width: rule.pixel_width,
                bond_color: rule.color,
                show_polyhedra: rule.show_polyhedra,
                polyhedron_color: wyckoscope::elements::style_for_scheme(
                    &rule.center,
                    settings.element_color_scheme,
                )
                .color
                .to_srgba()
                .to_u8_array_no_alpha(),
                polyhedron_opacity: BondRule::default().polyhedron_opacity,
            })
            .collect();
        settings
            .vectors
            .layers
            .extend(visuals.vectors.iter().filter_map(|vector| {
                let atom = structure
                    .asymmetric_atoms
                    .iter()
                    .find(|atom| atom.label == vector.label)?;
                Some(VectorLayer::imported_site(
                    atom.site_id,
                    vector.components.to_array(),
                    vector.color,
                    vector.scale,
                    vector.penetrate_atom,
                    vector.add_atom_radius,
                ))
            }));
    }
}
