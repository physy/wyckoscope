use std::{
    path::Path,
    sync::{Mutex, mpsc},
};

use bevy::{prelude::*, tasks::IoTaskPool, window::FileDragAndDrop};
use wyckoscope::parser;

use crate::state::*;

pub(crate) fn load_dropped_file(
    mut drops: MessageReader<FileDragAndDrop>,
    mut state: ResMut<AppState>,
    mut dirty: ResMut<SceneDirty>,
    mut selection: ResMut<SelectionState>,
    mut settings: ResMut<VisualSettings>,
) {
    for event in drops.read() {
        if let FileDragAndDrop::DroppedFile { path_buf, .. } = event {
            load_file(
                path_buf,
                &mut state,
                &mut settings,
                &mut dirty,
                &mut selection,
            );
        }
    }
}

pub(crate) fn keyboard_shortcuts(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut dialogs: ResMut<AsyncDialogs>,
) {
    if keyboard.just_pressed(KeyCode::KeyO) {
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

pub(crate) fn poll_async_dialogs(
    mut dialogs: ResMut<AsyncDialogs>,
    mut state: ResMut<AppState>,
    mut dirty: ResMut<SceneDirty>,
    mut selection: ResMut<SelectionState>,
    mut settings: ResMut<VisualSettings>,
    mut export: ResMut<ExportState>,
) {
    let open = dialogs
        .open_receiver
        .as_ref()
        .and_then(|receiver| receiver.lock().ok().and_then(|value| value.try_recv().ok()));
    if let Some(path) = open {
        dialogs.open_receiver = None;
        if let Some(path) = path {
            load_file(&path, &mut state, &mut settings, &mut dirty, &mut selection);
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
}

fn load_file(
    path: &Path,
    state: &mut AppState,
    settings: &mut VisualSettings,
    dirty: &mut SceneDirty,
    selection: &mut SelectionState,
) {
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
        }
        Err(error) => state.error = Some(error.to_string()),
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
            atom.magnetic_moment.map(|moment| {
                VectorLayer::magnetic_moment_site(atom.label.clone(), atom.element.clone(), moment)
            })
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
                    vector.label.clone(),
                    atom.element.clone(),
                    vector.components.to_array(),
                    vector.color,
                    vector.scale,
                    vector.penetrate_atom,
                    vector.add_atom_radius,
                ))
            }));
    }
}
