use std::{
    path::Path,
    sync::{Mutex, mpsc},
};

use bevy::{prelude::*, tasks::IoTaskPool, window::FileDragAndDrop};
use wyckoscope::parser;

use crate::{
    project::{ProjectHistory, UndoRedoRequests, capture_document, read_project, write_project},
    state::*,
};

pub(crate) fn load_dropped_file(
    mut drops: MessageReader<FileDragAndDrop>,
    mut state: ResMut<AppState>,
    mut dirty: ResMut<SceneDirty>,
    mut selection: ResMut<SelectionState>,
    mut settings: ResMut<VisualSettings>,
    mut history: ResMut<ProjectHistory>,
) {
    for event in drops.read() {
        if let FileDragAndDrop::DroppedFile { path_buf, .. } = event
            && load_file(
                path_buf,
                &mut state,
                &mut settings,
                &mut dirty,
                &mut selection,
            )
        {
            history.clear();
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
    mut dirty: ResMut<SceneDirty>,
    mut selection: ResMut<SelectionState>,
    mut settings: ResMut<VisualSettings>,
    mut overrides: ResMut<ElementOverrides>,
    mut export: ResMut<ExportState>,
    mut camera: Single<(&mut OrbitCamera, &mut Transform), With<OrbitCamera>>,
    mut history: ResMut<ProjectHistory>,
) {
    let open = dialogs
        .open_receiver
        .as_ref()
        .and_then(|receiver| receiver.lock().ok().and_then(|value| value.try_recv().ok()));
    if let Some(path) = open {
        dialogs.open_receiver = None;
        if let Some(path) = path
            && load_file(&path, &mut state, &mut settings, &mut dirty, &mut selection)
        {
            history.clear();
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
            let (orbit, transform) = &mut *camera;
            if let Err(error) = read_project(
                &path,
                &mut state,
                &mut settings,
                &mut overrides,
                &mut selection,
                &mut dirty,
                orbit,
                transform,
                &mut history,
            ) {
                state.error = Some(error);
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
    settings: &mut VisualSettings,
    dirty: &mut SceneDirty,
    selection: &mut SelectionState,
) -> bool {
    match parser::parse_path(path) {
        Ok(structure) => {
            apply_imported_visuals(settings, &structure);
            state.structure = structure;
            state.file_name = path
                .file_name()
                .and_then(|v| v.to_str())
                .unwrap_or("Structure")
                .to_string();
            state.error = None;
            selection.atoms.clear();
            dirty.mark(true);
            true
        }
        Err(error) => {
            state.error = Some(error.to_string());
            false
        }
    }
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
        settings.bond_match_mode = BondMatchMode::Element;
        settings.bond_rules = visuals
            .bond_rules
            .iter()
            .map(|rule| BondRule {
                enabled: true,
                show_bonds: true,
                search_mode: BondSearchMode::A2BondedToA1,
                boundary_mode: BondBoundaryMode::SearchAdditionalForA1,
                center: rule.center.clone(),
                neighbor: rule.neighbor.clone(),
                min_length: rule.min_length,
                max_length: rule.max_length,
                show_polyhedra: rule.show_polyhedra,
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
