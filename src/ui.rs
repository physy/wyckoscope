use std::collections::HashSet;

use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};

use crate::{
    AXIS_VIEW_MARGIN, AXIS_VIEW_SIZE,
    file_io::{request_open_dialog, request_save_dialog},
    scene::{boundary_image_count, crystal_to_world, default_appearance, view_directions},
    state::*,
};

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(crate) fn interface(
    mut contexts: EguiContexts,
    mut state: ResMut<AppState>,
    mut settings: ResMut<VisualSettings>,
    mut overrides: ResMut<ElementOverrides>,
    mut dialogs: ResMut<AsyncDialogs>,
    export: Res<ExportState>,
    scene_status: Res<SceneStatus>,
    mut selection: ResMut<SelectionState>,
    mut dirty: ResMut<SceneDirty>,
    mut layout: ResMut<InspectorLayout>,
    mut camera: Single<(&mut OrbitCamera, &Transform), With<OrbitCamera>>,
) -> Result {
    let ctx = contexts.ctx_mut()?;
    configure_egui_style(ctx);
    let panel = egui::SidePanel::left("inspector")
        .resizable(true)
        .default_width(layout.width)
        .min_width(260.0)
        .max_width(560.0)
        .frame(
            egui::Frame::NONE
                .fill(egui::Color32::from_rgba_unmultiplied(10, 14, 25, 248))
                .inner_margin(egui::Margin::same(18)),
        )
        .show(ctx, |ui| {
            app_header(ui);
            ui.add_space(18.0);
            tab_bar(ui, &mut settings.tab);
            panel_divider(ui);
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| match settings.tab {
                    InspectorTab::Structure => ui_structure_tab(
                        ui,
                        &state,
                        &mut settings,
                        &overrides,
                        &mut dialogs,
                        &scene_status,
                        &mut selection,
                        &mut dirty,
                    ),
                    InspectorTab::View => {
                        let (orbit, _) = &mut *camera;
                        ui_view_tab(ui, &state, &mut settings, orbit, &mut dirty);
                    }
                    InspectorTab::Elements => {
                        ui_elements_tab(ui, &state, &mut overrides, &mut dirty)
                    }
                    InspectorTab::Export => ui_export_tab(ui, &mut settings, &mut dialogs, &export),
                });
        });
    layout.width = panel.response.rect.width();
    let (_, camera_transform) = &*camera;
    draw_orientation_labels(ctx, &state, &settings, camera_transform, layout.width);
    if let Some(error) = state.error.clone() {
        egui::Window::new("Could not open structure")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label(error);
                if ui.button("Dismiss").clicked() {
                    state.error = None;
                }
            });
    }
    Ok(())
}

fn draw_orientation_labels(
    ctx: &egui::Context,
    state: &AppState,
    settings: &VisualSettings,
    camera_transform: &Transform,
    panel_width: f32,
) {
    egui::Area::new(egui::Id::new("orientation_axes"))
        .anchor(
            egui::Align2::LEFT_BOTTOM,
            egui::vec2(panel_width + AXIS_VIEW_MARGIN, -AXIS_VIEW_MARGIN),
        )
        .interactable(false)
        .show(ctx, |ui| {
            let (rect, _) = ui.allocate_exact_size(
                egui::vec2(AXIS_VIEW_SIZE, AXIS_VIEW_SIZE),
                egui::Sense::hover(),
            );
            // Labels deliberately paint outside the 3D axis viewport.  The viewport is
            // compact, while large labels need room around it to remain fully visible.
            ui.set_clip_rect(ctx.content_rect());
            let painter = ui.painter();
            let origin = rect.center();
            let axes = state.structure.cell.vectors.map(crystal_to_world);
            let label_extent = settings.axis_label_size * 0.65 + 7.0;
            let content_rect = ctx.content_rect();
            let label_min = egui::pos2(
                (panel_width + AXIS_VIEW_MARGIN).max(content_rect.min.x) + label_extent,
                content_rect.min.y + label_extent,
            );
            let label_max = egui::pos2(
                (content_rect.max.x - label_extent).max(label_min.x),
                (content_rect.max.y - label_extent).max(label_min.y),
            );
            let font =
                egui::FontId::new(settings.axis_label_size, settings.axis_label_font.family());
            let outline_width = (settings.axis_label_size * 0.08).clamp(1.0, 2.5);
            let outline_offsets = [
                egui::vec2(-outline_width, 0.0),
                egui::vec2(outline_width, 0.0),
                egui::vec2(0.0, -outline_width),
                egui::vec2(0.0, outline_width),
                egui::vec2(-outline_width, -outline_width),
                egui::vec2(-outline_width, outline_width),
                egui::vec2(outline_width, -outline_width),
                egui::vec2(outline_width, outline_width),
            ];

            for (index, label) in ["a", "b", "c"].into_iter().enumerate() {
                let view_direction =
                    camera_transform.rotation.inverse() * axes[index].normalize_or_zero();
                let projected = egui::vec2(view_direction.x, -view_direction.y);
                let projected_length = projected.length();
                let direction = if projected_length > 1.0e-4 {
                    projected / projected_length
                } else {
                    egui::vec2(0.0, -1.0)
                };
                let endpoint = origin + projected * (AXIS_VIEW_SIZE / 3.4 * 1.2);
                let color = egui::Color32::from_rgb(
                    settings.axis_colors[index][0],
                    settings.axis_colors[index][1],
                    settings.axis_colors[index][2],
                );
                let label_position = (endpoint
                    + direction * (settings.axis_label_size * 0.55 + 7.0))
                    .clamp(label_min, label_max);
                for offset in outline_offsets {
                    painter.text(
                        label_position + offset,
                        egui::Align2::CENTER_CENTER,
                        label,
                        font.clone(),
                        egui::Color32::WHITE,
                    );
                }
                painter.text(
                    label_position,
                    egui::Align2::CENTER_CENTER,
                    label,
                    font.clone(),
                    color,
                );
            }
        });
}

fn app_header(ui: &mut egui::Ui) {
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(34.0, 34.0), egui::Sense::hover());
        ui.painter()
            .circle_filled(rect.center(), 16.0, egui::Color32::from_rgb(65, 174, 255));
        ui.painter().circle_filled(
            rect.center() + egui::vec2(5.0, -4.0),
            6.0,
            egui::Color32::from_rgb(255, 106, 88),
        );
        ui.vertical(|ui| {
            ui.label(
                egui::RichText::new("Wyckoscope")
                    .size(23.0)
                    .strong()
                    .color(egui::Color32::WHITE),
            );
            ui.label(
                egui::RichText::new("CRYSTAL VIEWER")
                    .size(11.0)
                    .strong()
                    .color(egui::Color32::from_rgb(112, 154, 195)),
            );
        });
    });
}

fn tab_bar(ui: &mut egui::Ui, active: &mut InspectorTab) {
    egui::Grid::new("main_tabs")
        .num_columns(2)
        .spacing([6.0, 6.0])
        .show(ui, |ui| {
            tab_button(ui, active, InspectorTab::Structure, "Structure");
            tab_button(ui, active, InspectorTab::View, "View");
            ui.end_row();
            tab_button(ui, active, InspectorTab::Elements, "Elements");
            tab_button(ui, active, InspectorTab::Export, "Export");
            ui.end_row();
        });
}

fn tab_button(ui: &mut egui::Ui, active: &mut InspectorTab, tab: InspectorTab, label: &str) {
    if ui
        .add_sized(
            [145.0, 31.0],
            egui::Button::new(egui::RichText::new(label).strong()).selected(*active == tab),
        )
        .clicked()
    {
        *active = tab;
    }
}

#[allow(clippy::too_many_arguments)]
fn ui_structure_tab(
    ui: &mut egui::Ui,
    state: &AppState,
    settings: &mut VisualSettings,
    overrides: &ElementOverrides,
    dialogs: &mut AsyncDialogs,
    status: &SceneStatus,
    selection: &mut SelectionState,
    dirty: &mut SceneDirty,
) {
    ui.add_space(12.0);
    let opening = dialogs.open_receiver.is_some();
    if ui
        .add_enabled(
            !opening,
            egui::Button::new(if opening {
                "Opening file dialog…"
            } else {
                "Open structure"
            })
            .min_size(egui::vec2(ui.available_width(), 38.0)),
        )
        .clicked()
    {
        request_open_dialog(dialogs);
    }
    ui.label(
        egui::RichText::new("or drop .cif, .mcif, or .vesta anywhere")
            .size(11.0)
            .color(egui::Color32::from_rgb(106, 121, 145)),
    );
    section_header(ui, "STRUCTURE");
    ui.label(
        egui::RichText::new(&state.structure.title)
            .size(16.0)
            .strong()
            .color(egui::Color32::from_rgb(230, 238, 248)),
    );
    ui.horizontal_wrapped(|ui| {
        pill(ui, state.structure.source_format.label());
        ui.label(
            egui::RichText::new(&state.file_name)
                .size(11.0)
                .color(egui::Color32::from_rgb(127, 143, 166)),
        );
    });
    ui.add_space(12.0);
    egui::Grid::new("structure_stats")
        .num_columns(2)
        .spacing([18.0, 7.0])
        .show(ui, |ui| {
            stat(
                ui,
                "Sites / expanded",
                format!(
                    "{} / {}",
                    state.structure.asymmetric_atoms.len(),
                    state.structure.atoms.len()
                ),
            );
            stat(
                ui,
                "Space group",
                state
                    .structure
                    .space_group
                    .clone()
                    .unwrap_or_else(|| "not specified".to_string()),
            );
            stat(
                ui,
                "Operations",
                state.structure.symmetry_operation_count.to_string(),
            );
            stat(
                ui,
                "Volume",
                format!("{:.2} Å³", state.structure.cell.volume()),
            );
            stat(
                ui,
                "Cell",
                format!(
                    "{:.2} × {:.2} × {:.2} Å",
                    state.structure.cell.a, state.structure.cell.b, state.structure.cell.c
                ),
            );
            stat(
                ui,
                "Angles",
                format!(
                    "{:.0}°  {:.0}°  {:.0}°",
                    state.structure.cell.alpha,
                    state.structure.cell.beta,
                    state.structure.cell.gamma
                ),
            );
        });
    section_header(ui, "SYMMETRY");
    if ui
        .checkbox(
            &mut settings.expand_symmetry,
            "Expand atoms with space-group operations",
        )
        .changed()
    {
        dirty.mark(true);
    }
    section_header(ui, "SUPERCELL");
    ui.horizontal(|ui| {
        for (axis, value) in ["A", "B", "C"].into_iter().zip(&mut settings.supercell) {
            ui.vertical(|ui| {
                ui.label(
                    egui::RichText::new(axis)
                        .size(11.0)
                        .color(egui::Color32::from_rgb(107, 130, 158)),
                );
                if ui
                    .add(egui::DragValue::new(value).range(1..=u32::MAX).speed(0.1))
                    .changed()
                {
                    *value = (*value).max(1);
                    dirty.mark(true);
                }
            });
            ui.add_space(10.0);
        }
    });
    if ui
        .checkbox(
            &mut settings.show_boundary_atoms,
            "Show periodic atoms on outer cell faces",
        )
        .changed()
    {
        dirty.mark(true);
    }
    let source = state
        .structure
        .visible_atoms(settings.expand_symmetry)
        .iter()
        .filter(|atom| {
            overrides
                .0
                .get(&atom.element)
                .is_none_or(|appearance| appearance.visible)
        })
        .count() as u64;
    let supercell_count = settings
        .supercell
        .iter()
        .fold(source, |n, value| n.saturating_mul(*value as u64));
    let boundary_images = settings.show_boundary_atoms.then(|| {
        state
            .structure
            .visible_atoms(settings.expand_symmetry)
            .iter()
            .filter(|atom| {
                overrides
                    .0
                    .get(&atom.element)
                    .is_none_or(|appearance| appearance.visible)
            })
            .map(|atom| boundary_image_count(atom.fractional).saturating_sub(1))
            .sum::<u64>()
    });
    let count = supercell_count.saturating_add(boundary_images.unwrap_or_default());
    ui.label(
        egui::RichText::new(format!("{count} displayed atoms"))
            .size(11.0)
            .color(egui::Color32::from_rgb(127, 143, 166)),
    );
    if let Some(warning) = &status.warning {
        ui.colored_label(egui::Color32::from_rgb(244, 178, 76), warning);
    }
    ui_selection_controls(ui, selection, dirty);
    ui_atom_vector_controls(ui, state, settings, selection, dirty);
    if !state.structure.warnings.is_empty() {
        section_header(ui, "IMPORT NOTES");
        for warning in &state.structure.warnings {
            ui.label(
                egui::RichText::new(warning)
                    .size(11.0)
                    .color(egui::Color32::from_rgb(180, 161, 126)),
            );
        }
    }
}

fn ui_selection_controls(
    ui: &mut egui::Ui,
    selection: &mut SelectionState,
    dirty: &mut SceneDirty,
) {
    section_header(ui, "SELECTION & MEASUREMENT");
    ui.label(
        egui::RichText::new(
            "Click an atom to select it. Shift-click adds or removes atoms (up to 3).",
        )
        .size(11.0)
        .color(egui::Color32::from_rgb(127, 143, 166)),
    );
    if selection.atoms.is_empty() {
        ui.label(
            egui::RichText::new("Select an atom in the 3D view to inspect its coordination.")
                .size(11.0)
                .color(egui::Color32::from_rgb(158, 174, 195)),
        );
        return;
    }
    ui.add_space(5.0);
    for (index, atom) in selection.atoms.iter().enumerate() {
        egui::Frame::NONE
            .fill(egui::Color32::from_rgb(22, 31, 50))
            .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(48, 65, 94)))
            .corner_radius(6.0)
            .inner_margin(egui::Margin::symmetric(8, 6))
            .show(ui, |ui| {
                ui.label(
                    egui::RichText::new(format!(
                        "{}  {} ({})",
                        index + 1,
                        atom.element,
                        atom.label
                    ))
                    .size(12.0)
                    .strong()
                    .color(egui::Color32::from_rgb(245, 199, 91)),
                );
                ui.label(
                    egui::RichText::new(format!(
                        "Fractional: {:.4}, {:.4}, {:.4} · coordination: {}",
                        atom.fractional.x, atom.fractional.y, atom.fractional.z, atom.coordination
                    ))
                    .size(11.0)
                    .color(egui::Color32::from_rgb(191, 204, 223)),
                );
            });
        ui.add_space(4.0);
    }
    if let [first, second, ..] = selection.atoms.as_slice() {
        ui.label(
            egui::RichText::new(format!(
                "Distance (1–2): {:.4} Å",
                first.position.distance(second.position)
            ))
            .size(12.0)
            .strong()
            .color(egui::Color32::from_rgb(125, 203, 255)),
        );
    }
    if let [first, vertex, third] = selection.atoms.as_slice() {
        let left = (first.position - vertex.position).normalize_or_zero();
        let right = (third.position - vertex.position).normalize_or_zero();
        if left.length_squared() > 0.0 && right.length_squared() > 0.0 {
            ui.label(
                egui::RichText::new(format!(
                    "Angle (1–2–3): {:.3}°",
                    left.dot(right).clamp(-1.0, 1.0).acos().to_degrees()
                ))
                .size(12.0)
                .strong()
                .color(egui::Color32::from_rgb(125, 203, 255)),
            );
        }
    }
    if ui.button("Clear selection").clicked() {
        selection.atoms.clear();
        dirty.mark(false);
    }
}

fn ui_atom_vector_controls(
    ui: &mut egui::Ui,
    state: &AppState,
    settings: &mut VisualSettings,
    selection: &SelectionState,
    dirty: &mut SceneDirty,
) {
    section_header(ui, "ATOMIC VECTORS / SPINS");
    ui.label(
        egui::RichText::new(
            "Axial vectors represent magnetic moments. Site vectors follow spatial symmetry.",
        )
        .size(11.0)
        .color(egui::Color32::from_rgb(127, 143, 166)),
    );
    let imported_count = state
        .structure
        .atoms
        .iter()
        .filter(|atom| atom.magnetic_moment.is_some())
        .count();
    if imported_count > 0 {
        ui.label(
            egui::RichText::new(format!("Loaded {imported_count} mCIF magnetic moment(s)."))
                .size(11.0)
                .color(egui::Color32::from_rgb(125, 203, 255)),
        );
        if ui
            .checkbox(&mut settings.show_imported_moments, "Show imported moments")
            .changed()
        {
            dirty.mark(false);
        }
        if settings.show_imported_moments {
            ui.horizontal(|ui| {
                ui.label("Imported moment color");
                if ui
                    .color_edit_button_srgb(&mut settings.imported_moment_color)
                    .changed()
                {
                    dirty.mark(false);
                }
            });
            if labeled_slider(
                ui,
                "Imported moment scale",
                &mut settings.imported_moment_scale,
                0.05..=2.5,
            ) {
                dirty.mark(false);
            }
        }
        panel_divider(ui);
    }
    if let Some(atom) = selection.atoms.first() {
        ui.horizontal_wrapped(|ui| {
            if ui
                .button(format!("Add site vector: {}", atom.label))
                .clicked()
            {
                settings.atom_vectors.push(AtomVector::for_site(atom));
                dirty.mark(false);
            }
            if ui
                .button(format!("Add selected atom: {}", atom.label))
                .clicked()
            {
                settings.atom_vectors.push(AtomVector::for_atom(atom));
                dirty.mark(false);
            }
        });
    } else {
        ui.label(
            egui::RichText::new("Select an atom first to add a site-wide or individual vector.")
                .size(11.0)
                .color(egui::Color32::from_rgb(158, 174, 195)),
        );
    }
    let mut remove = None;
    for (index, vector) in settings.atom_vectors.iter_mut().enumerate() {
        egui::Frame::NONE
            .fill(egui::Color32::from_rgb(22, 31, 50))
            .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(48, 65, 94)))
            .corner_radius(8.0)
            .inner_margin(egui::Margin::same(9))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(format!(
                            "{} vector · {} {}",
                            vector.attachment.label(),
                            vector.element,
                            vector.label
                        ))
                        .size(12.0)
                        .strong()
                        .color(egui::Color32::from_rgb(245, 199, 91)),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Remove").clicked() {
                            remove = Some(index);
                        }
                    });
                });
                if vector3_control(ui, "Moment (u, v, w)", &mut vector.components, 0.05) {
                    dirty.mark(false);
                }
                if labeled_slider(ui, "Scale", &mut vector.scale, 0.05..=2.5) {
                    dirty.mark(false);
                }
                ui.horizontal(|ui| {
                    ui.label("Color");
                    if ui.color_edit_button_srgb(&mut vector.color).changed() {
                        dirty.mark(false);
                    }
                    if ui
                        .checkbox(&mut vector.penetrate_atom, "Penetrate atom")
                        .changed()
                    {
                        dirty.mark(false);
                    }
                });
                if ui
                    .checkbox(&mut vector.add_atom_radius, "Offset by atom radius")
                    .changed()
                {
                    dirty.mark(false);
                }
            });
        ui.add_space(7.0);
    }
    if let Some(index) = remove {
        settings.atom_vectors.remove(index);
        dirty.mark(false);
    }
}

fn ui_view_tab(
    ui: &mut egui::Ui,
    state: &AppState,
    settings: &mut VisualSettings,
    orbit: &mut OrbitCamera,
    dirty: &mut SceneDirty,
) {
    section_header(ui, "CAMERA DIRECTION");
    egui::Grid::new("view_directions")
        .num_columns(4)
        .spacing([6.0, 6.0])
        .show(ui, |ui| {
            for (index, (label, direction)) in
                view_directions(&state.structure).into_iter().enumerate()
            {
                if ui
                    .add_sized([64.0, 29.0], egui::Button::new(label))
                    .clicked()
                {
                    orbit.look_from(direction);
                }
                if index == 3 {
                    ui.end_row();
                }
            }
        });
    if ui
        .add_sized(
            [ui.available_width(), 31.0],
            egui::Button::new("Reset camera   F"),
        )
        .clicked()
    {
        orbit.reset();
    }
    ui.label(
        egui::RichText::new("Left drag: free rotate · Shift/right drag: move · Wheel: zoom")
            .size(11.0)
            .color(egui::Color32::from_rgb(112, 134, 160)),
    );
    section_header(ui, "PROJECTION");
    ui.horizontal(|ui| {
        if ui.button("Strong").clicked() {
            settings.projection_blend = -1.0;
        }
        ui.add(
            egui::Slider::new(&mut settings.projection_blend, -1.0..=1.0)
                .show_value(false)
                .custom_formatter(|value, _| format!("{:.0}%", value * 100.0)),
        );
        if ui.button("Parallel").clicked() {
            settings.projection_blend = 1.0;
        }
    });
    if ui
        .add_sized(
            [ui.available_width(), 27.0],
            egui::Button::new("Standard perspective"),
        )
        .clicked()
    {
        settings.projection_blend = 0.0;
    }
    ui.label(
        egui::RichText::new("Strong perspective  <  standard  >  parallel")
            .size(11.0)
            .color(egui::Color32::from_rgb(112, 134, 160)),
    );
    section_header(ui, "ORIENTATION AXES");
    ui.horizontal(|ui| {
        for (label, color) in ["a", "b", "c"].into_iter().zip(&mut settings.axis_colors) {
            ui.label(label);
            ui.color_edit_button_srgb(color);
            ui.add_space(6.0);
        }
    });
    labeled_slider(ui, "Label size", &mut settings.axis_label_size, 11.0..=36.0);
    ui.label(
        egui::RichText::new("Label font")
            .size(11.0)
            .color(egui::Color32::from_rgb(158, 174, 195)),
    );
    ui.horizontal(|ui| {
        ui.radio_value(
            &mut settings.axis_label_font,
            AxisLabelFont::SansSerif,
            AxisLabelFont::SansSerif.label(),
        );
        ui.radio_value(
            &mut settings.axis_label_font,
            AxisLabelFont::Monospace,
            AxisLabelFont::Monospace.label(),
        );
    });
    section_header(ui, "BACKGROUND");
    ui.horizontal(|ui| {
        ui.label("Background");
        if ui
            .color_edit_button_srgb(&mut settings.background)
            .changed()
        {
            dirty.mark(false);
        }
        if ui.button("White").clicked() {
            settings.background = [255, 255, 255];
            dirty.mark(false);
        }
        if ui.button("Dark").clicked() {
            settings.background = [4, 7, 14];
            dirty.mark(false);
        }
    });
    ui_lighting_controls(
        ui,
        &mut settings.lighting,
        &mut settings.exposure_ev,
        &mut settings.hdr_tonemapping,
    );
    section_header(ui, "APPEARANCE");
    if labeled_slider(ui, "Global atom scale", &mut settings.atom_scale, 0.4..=2.0) {
        dirty.mark(false);
    }
    if labeled_slider(ui, "Bond width", &mut settings.bond_radius, 0.025..=0.24) {
        dirty.mark(false);
    }
    if labeled_slider(
        ui,
        "Default bond cutoff",
        &mut settings.bond_cutoff,
        1.0..=5.0,
    ) {
        dirty.mark(false);
    }
    ui_bond_rule_controls(ui, settings, dirty);
    ui.horizontal(|ui| {
        ui.label("Atoms");
        egui::ComboBox::from_id_salt("atom_display_mode")
            .selected_text(settings.atom_display_mode.label())
            .show_ui(ui, |ui| {
                for mode in [
                    AtomDisplayMode::BallAndStick,
                    AtomDisplayMode::SpaceFilling,
                    AtomDisplayMode::Wireframe,
                ] {
                    if ui
                        .selectable_value(&mut settings.atom_display_mode, mode, mode.label())
                        .changed()
                    {
                        dirty.mark(false);
                    }
                }
            });
    });
    if ui
        .checkbox(
            &mut settings.show_polyhedra,
            "Coordination polyhedron for selected atom",
        )
        .changed()
    {
        dirty.mark(false);
    }
    if settings.show_polyhedra {
        ui.horizontal(|ui| {
            ui.label("Polyhedron color");
            if ui
                .color_edit_button_srgb(&mut settings.polyhedron_color)
                .changed()
            {
                dirty.mark(false);
            }
        });
        if labeled_slider(
            ui,
            "Polyhedron opacity",
            &mut settings.polyhedron_opacity,
            0.05..=0.85,
        ) {
            dirty.mark(false);
        }
    }
    ui_unit_cell_controls(ui, settings, dirty);
    ui.horizontal(|ui| {
        ui.label("Ground color");
        if ui
            .color_edit_button_srgb(&mut settings.ground_color)
            .changed()
        {
            dirty.mark(false);
        }
    });
    egui::Grid::new("view_toggles")
        .num_columns(2)
        .spacing([18.0, 8.0])
        .show(ui, |ui| {
            if ui.checkbox(&mut settings.show_bonds, "Bonds").changed() {
                dirty.mark(false);
            }
            ui.label("");
            ui.end_row();
            if ui
                .checkbox(&mut settings.show_outlines, "Outlines")
                .changed()
            {
                dirty.mark(false);
            }
            if ui.checkbox(&mut settings.show_floor, "Ground").changed() {
                dirty.mark(false);
            }
            ui.end_row();
            ui.checkbox(&mut settings.auto_rotate, "Auto rotate");
            ui.label("");
        });
}

fn ui_unit_cell_controls(ui: &mut egui::Ui, settings: &mut VisualSettings, dirty: &mut SceneDirty) {
    section_header(ui, "UNIT CELL");
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.label(
                egui::RichText::new("Line")
                    .size(11.0)
                    .color(egui::Color32::from_rgb(158, 174, 195)),
            );
            for display in [
                UnitCellDisplay::Hidden,
                UnitCellDisplay::Single,
                UnitCellDisplay::All,
            ] {
                if ui
                    .radio_value(&mut settings.unit_cell_display, display, display.label())
                    .changed()
                {
                    dirty.mark(false);
                }
            }
        });
        ui.add_space(18.0);
        ui.vertical(|ui| {
            ui.label(
                egui::RichText::new("Line style")
                    .size(11.0)
                    .color(egui::Color32::from_rgb(158, 174, 195)),
            );
            for style in [
                CellLineStyle::Solid,
                CellLineStyle::Dotted,
                CellLineStyle::Dashed,
            ] {
                if ui
                    .radio_value(&mut settings.unit_cell_line_style, style, style.label())
                    .changed()
                {
                    dirty.mark(false);
                }
            }
        });
    });
    ui.horizontal(|ui| {
        ui.label("Line width");
        if ui
            .add(
                egui::DragValue::new(&mut settings.unit_cell_line_width)
                    .range(0.25..=8.0)
                    .speed(0.05)
                    .max_decimals(2),
            )
            .changed()
        {
            dirty.mark(false);
        }
        ui.label("Color");
        if ui
            .color_edit_button_srgb(&mut settings.unit_cell_color)
            .changed()
        {
            dirty.mark(false);
        }
    });
}

fn ui_bond_rule_controls(ui: &mut egui::Ui, settings: &mut VisualSettings, dirty: &mut SceneDirty) {
    section_header(ui, "BOND RULES & POLYHEDRA");
    ui.horizontal(|ui| {
        ui.label("Match by");
        for mode in [BondMatchMode::Element, BondMatchMode::SiteLabel] {
            if ui
                .radio_value(&mut settings.bond_match_mode, mode, mode.label())
                .changed()
            {
                dirty.mark(false);
            }
        }
    });
    ui.label(
        egui::RichText::new(
            "Rules override automatic bond detection. Use * or Any to match every element/site.",
        )
        .size(11.0)
        .color(egui::Color32::from_rgb(127, 143, 166)),
    );
    if ui.button("Add bond rule").clicked() {
        settings.bond_rules.push(BondRule::default());
        dirty.mark(false);
    }
    let mut remove = None;
    for (index, rule) in settings.bond_rules.iter_mut().enumerate() {
        egui::Frame::NONE
            .fill(egui::Color32::from_rgb(22, 31, 50))
            .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(48, 65, 94)))
            .corner_radius(8.0)
            .inner_margin(egui::Margin::same(9))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if ui.checkbox(&mut rule.enabled, "Enabled").changed() {
                        dirty.mark(false);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Remove").clicked() {
                            remove = Some(index);
                        }
                    });
                });
                egui::Grid::new(egui::Id::new(("bond_rule", index)))
                    .num_columns(2)
                    .spacing([8.0, 6.0])
                    .show(ui, |ui| {
                        ui.label("Center A1");
                        if ui.text_edit_singleline(&mut rule.center).changed() {
                            dirty.mark(false);
                        }
                        ui.end_row();
                        ui.label("Neighbor A2");
                        if ui.text_edit_singleline(&mut rule.neighbor).changed() {
                            dirty.mark(false);
                        }
                        ui.end_row();
                        ui.label("Min (Å)");
                        if ui
                            .add(egui::DragValue::new(&mut rule.min_length).range(0.0..=50.0))
                            .changed()
                        {
                            dirty.mark(false);
                        }
                        ui.end_row();
                        ui.label("Max (Å)");
                        if ui
                            .add(egui::DragValue::new(&mut rule.max_length).range(0.0..=50.0))
                            .changed()
                        {
                            rule.max_length = rule.max_length.max(rule.min_length);
                            dirty.mark(false);
                        }
                        ui.end_row();
                    });
                if ui
                    .checkbox(
                        &mut rule.show_polyhedra,
                        "Generate polyhedra with A1 as center",
                    )
                    .changed()
                {
                    dirty.mark(false);
                }
            });
        ui.add_space(7.0);
    }
    if settings.bond_rules.iter().any(|rule| rule.show_polyhedra) {
        ui.horizontal(|ui| {
            ui.label("Polyhedron color");
            if ui
                .color_edit_button_srgb(&mut settings.polyhedron_color)
                .changed()
            {
                dirty.mark(false);
            }
        });
        if labeled_slider(
            ui,
            "Polyhedron opacity",
            &mut settings.polyhedron_opacity,
            0.05..=0.85,
        ) {
            dirty.mark(false);
        }
    }
}

fn ui_lighting_controls(
    ui: &mut egui::Ui,
    lighting: &mut LightingSettings,
    exposure_ev: &mut f32,
    hdr_tonemapping: &mut bool,
) {
    if ui
        .add_sized(
            [ui.available_width(), 28.0],
            egui::Button::new("Reset all lighting"),
        )
        .clicked()
    {
        *lighting = LightingSettings::default();
        *exposure_ev = 7.5;
        *hdr_tonemapping = true;
    }

    section_header(ui, "AMBIENT LIGHT");
    ui.checkbox(&mut lighting.ambient_enabled, "Enabled");
    color_control(ui, "Color", &mut lighting.ambient_color);
    labeled_slider(
        ui,
        "Brightness (cd/m²)",
        &mut lighting.ambient_brightness,
        0.0..=5_000.0,
    );

    section_header(ui, "KEY LIGHT");
    ui.checkbox(&mut lighting.key_enabled, "Enabled");
    ui.horizontal(|ui| {
        ui.label("Type");
        ui.radio_value(&mut lighting.key_kind, KeyLightKind::Point, "Point");
        ui.radio_value(&mut lighting.key_kind, KeyLightKind::Spot, "Spot");
    });
    color_control(ui, "Color", &mut lighting.key_color);
    labeled_slider(
        ui,
        "Intensity (lm)",
        &mut lighting.key_intensity,
        0.0..=5_000_000.0,
    );
    labeled_slider(ui, "Range", &mut lighting.key_range, 1.0..=500.0);
    labeled_slider(ui, "Source radius", &mut lighting.key_radius, 0.0..=20.0);
    ui.horizontal(|ui| {
        ui.radio_value(&mut lighting.key_mode, LightMode::Camera, "Follow camera");
        ui.radio_value(&mut lighting.key_mode, LightMode::World, "Fixed position");
    });
    match lighting.key_mode {
        LightMode::Camera => {
            labeled_slider(
                ui,
                "Light distance",
                &mut lighting.key_follow_distance,
                0.1..=200.0,
            );
            labeled_slider(
                ui,
                "Azimuth around camera (°)",
                &mut lighting.key_follow_azimuth,
                -180.0..=180.0,
            );
            labeled_slider(
                ui,
                "Elevation around camera (°)",
                &mut lighting.key_follow_elevation,
                -89.0..=89.0,
            );
            ui.label(
                egui::RichText::new("Zoom does not change the light distance or brightness.")
                    .size(11.0)
                    .color(egui::Color32::from_rgb(151, 170, 195)),
            );
        }
        LightMode::World => {
            let _ = vector3_control(ui, "Position", &mut lighting.key_position, 0.1);
        }
    }
    if lighting.key_kind == KeyLightKind::Spot {
        let _ = vector3_control(ui, "Target offset", &mut lighting.key_target_offset, 0.1);
        let outer_changed = labeled_slider(
            ui,
            "Outer angle (°)",
            &mut lighting.spot_outer_angle,
            1.0..=89.0,
        );
        let inner_changed = labeled_slider(
            ui,
            "Inner angle (°)",
            &mut lighting.spot_inner_angle,
            0.0..=89.0,
        );
        if outer_changed || inner_changed {
            lighting.spot_inner_angle = lighting.spot_inner_angle.min(lighting.spot_outer_angle);
        }
    }
    ui.checkbox(&mut lighting.key_shadows, "Cast shadows");
    egui::CollapsingHeader::new("Advanced shadow controls")
        .id_salt("key_shadow_advanced")
        .show(ui, |ui| {
            labeled_slider(
                ui,
                "Depth bias",
                &mut lighting.key_shadow_depth_bias,
                0.0..=1.0,
            );
            labeled_slider(
                ui,
                "Normal bias",
                &mut lighting.key_shadow_normal_bias,
                0.0..=5.0,
            );
            labeled_slider(
                ui,
                "Shadow near Z",
                &mut lighting.key_shadow_near_z,
                0.001..=10.0,
            );
        });

    section_header(ui, "FILL LIGHT");
    ui.checkbox(&mut lighting.fill_enabled, "Enabled");
    color_control(ui, "Color", &mut lighting.fill_color);
    labeled_slider(
        ui,
        "Illuminance (lux)",
        &mut lighting.fill_illuminance,
        0.0..=100_000.0,
    );
    let _ = vector3_control(ui, "Rotation XYZ (°)", &mut lighting.fill_rotation, 1.0);
    ui.checkbox(&mut lighting.fill_shadows, "Cast shadows");
    egui::CollapsingHeader::new("Fill shadow controls")
        .id_salt("fill_shadow_advanced")
        .show(ui, |ui| {
            labeled_slider(
                ui,
                "Depth bias",
                &mut lighting.fill_shadow_depth_bias,
                0.0..=1.0,
            );
            labeled_slider(
                ui,
                "Normal bias",
                &mut lighting.fill_shadow_normal_bias,
                0.0..=5.0,
            );
        });

    section_header(ui, "AXIS LIGHTING");
    ui.checkbox(&mut lighting.axis_key_enabled, "Key enabled");
    color_control(ui, "Key color", &mut lighting.axis_key_color);
    labeled_slider(
        ui,
        "Key illuminance",
        &mut lighting.axis_key_illuminance,
        0.0..=100_000.0,
    );
    let _ = vector3_control(
        ui,
        "Key rotation XYZ (°)",
        &mut lighting.axis_key_rotation,
        1.0,
    );
    ui.separator();
    ui.checkbox(&mut lighting.axis_fill_enabled, "Fill enabled");
    color_control(ui, "Fill color", &mut lighting.axis_fill_color);
    labeled_slider(
        ui,
        "Fill illuminance",
        &mut lighting.axis_fill_illuminance,
        0.0..=100_000.0,
    );
    let _ = vector3_control(
        ui,
        "Fill rotation XYZ (°)",
        &mut lighting.axis_fill_rotation,
        1.0,
    );

    section_header(ui, "CAMERA EXPOSURE");
    labeled_slider(ui, "Exposure EV", exposure_ev, 4.0..=13.0);
    ui.checkbox(hdr_tonemapping, "HDR tonemapping");
    ui.label(
        egui::RichText::new("Lower EV makes the scene brighter.")
            .size(11.0)
            .color(egui::Color32::from_rgb(112, 134, 160)),
    );
}

fn color_control(ui: &mut egui::Ui, label: &str, color: &mut [u8; 3]) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.color_edit_button_srgb(color);
    });
}

fn vector3_control(ui: &mut egui::Ui, label: &str, values: &mut [f32; 3], speed: f64) -> bool {
    ui.label(
        egui::RichText::new(label)
            .size(11.0)
            .color(egui::Color32::from_rgb(158, 174, 195)),
    );
    let mut changed = false;
    ui.horizontal(|ui| {
        for (axis, value) in ["X", "Y", "Z"].into_iter().zip(values) {
            ui.label(axis);
            changed |= ui
                .add(egui::DragValue::new(value).speed(speed).max_decimals(2))
                .changed();
        }
    });
    changed
}

fn ui_elements_tab(
    ui: &mut egui::Ui,
    state: &AppState,
    overrides: &mut ElementOverrides,
    dirty: &mut SceneDirty,
) {
    section_header(ui, "ELEMENT STYLES");
    ui.label(
        egui::RichText::new("Set the color and visual radius for each element.")
            .size(11.0)
            .color(egui::Color32::from_rgb(118, 138, 164)),
    );
    let mut symbols: Vec<_> = state
        .structure
        .atoms
        .iter()
        .map(|a| a.element.clone())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    symbols.sort();
    for symbol in symbols {
        let default = default_appearance(&symbol);
        let appearance = overrides.0.entry(symbol.clone()).or_insert(default.clone());
        egui::Frame::NONE
            .fill(egui::Color32::from_rgb(22, 31, 50))
            .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(48, 65, 94)))
            .corner_radius(8.0)
            .inner_margin(egui::Margin::same(10))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let title_color = if appearance.visible {
                        egui::Color32::from_rgb(242, 247, 255)
                    } else {
                        egui::Color32::from_rgb(143, 158, 181)
                    };
                    ui.label(
                        egui::RichText::new(&symbol)
                            .size(16.0)
                            .strong()
                            .color(title_color),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Reset").clicked() {
                            *appearance = default.clone();
                            dirty.mark(false);
                        }
                        if ui.color_edit_button_srgb(&mut appearance.color).changed() {
                            dirty.mark(false);
                        }
                        if ui.checkbox(&mut appearance.visible, "Visible").changed() {
                            dirty.mark(false);
                        }
                    });
                });
                if labeled_slider(ui, "Radius (Å)", &mut appearance.radius, 0.1..=2.5) {
                    dirty.mark(false);
                }
            });
        ui.add_space(7.0);
    }
    if ui
        .add_sized(
            [ui.available_width(), 32.0],
            egui::Button::new("Reset all element styles"),
        )
        .clicked()
    {
        overrides.0.clear();
        dirty.mark(false);
    }
}

fn ui_export_tab(
    ui: &mut egui::Ui,
    settings: &mut VisualSettings,
    dialogs: &mut AsyncDialogs,
    export: &ExportState,
) {
    section_header(ui, "HIGH-RESOLUTION PNG");
    ui.label(
        egui::RichText::new("Exports the 3D view without the inspector UI.")
            .size(11.0)
            .color(egui::Color32::from_rgb(118, 138, 164)),
    );
    ui.horizontal_wrapped(|ui| {
        for (label, size) in [
            ("Full HD", [1920, 1080]),
            ("4K", [3840, 2160]),
            ("Square", [4096, 4096]),
            ("Print", [6000, 4000]),
        ] {
            if ui.button(label).clicked() {
                settings.export_size = size;
            }
        }
    });
    egui::Grid::new("export_dimensions")
        .num_columns(2)
        .show(ui, |ui| {
            ui.label("Width");
            ui.add(
                egui::DragValue::new(&mut settings.export_size[0])
                    .range(256..=8192)
                    .suffix(" px"),
            );
            ui.end_row();
            ui.label("Height");
            ui.add(
                egui::DragValue::new(&mut settings.export_size[1])
                    .range(256..=8192)
                    .suffix(" px"),
            );
            ui.end_row();
        });
    let busy = dialogs.save_receiver.is_some() || export.stage.is_some() || export.capturing;
    ui.add_space(14.0);
    if ui
        .add_enabled(
            !busy,
            egui::Button::new(if busy { "Exporting…" } else { "Export PNG" })
                .min_size(egui::vec2(ui.available_width(), 40.0)),
        )
        .clicked()
    {
        request_save_dialog(dialogs, settings.export_size);
    }
    if let Some(message) = &export.message {
        ui.label(
            egui::RichText::new(message)
                .size(11.0)
                .color(egui::Color32::from_rgb(105, 190, 236)),
        );
    }
}

fn configure_egui_style(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(9.0, 8.0);
    style.visuals.dark_mode = true;
    style.visuals.override_text_color = Some(egui::Color32::from_rgb(231, 238, 249));
    style.visuals.panel_fill = egui::Color32::from_rgb(13, 19, 32);
    style.visuals.window_fill = egui::Color32::from_rgb(20, 29, 47);
    style.visuals.extreme_bg_color = egui::Color32::from_rgb(10, 15, 26);
    style.visuals.widgets.noninteractive.fg_stroke.color = egui::Color32::from_rgb(192, 205, 223);
    style.visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(29, 42, 66);
    style.visuals.widgets.inactive.weak_bg_fill = egui::Color32::from_rgb(23, 34, 54);
    style.visuals.widgets.inactive.fg_stroke.color = egui::Color32::from_rgb(224, 232, 245);
    style.visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(45, 69, 103);
    style.visuals.widgets.hovered.fg_stroke.color = egui::Color32::WHITE;
    style.visuals.widgets.active.bg_fill = egui::Color32::from_rgb(38, 126, 196);
    style.visuals.widgets.active.fg_stroke.color = egui::Color32::WHITE;
    style.visuals.selection.bg_fill = egui::Color32::from_rgb(45, 145, 218);
    style.visuals.widgets.inactive.corner_radius = 7.0.into();
    style.visuals.widgets.hovered.corner_radius = 7.0.into();
    style.visuals.widgets.active.corner_radius = 7.0.into();
    ctx.set_style(style);
}

fn section_header(ui: &mut egui::Ui, title: &str) {
    ui.add_space(15.0);
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(title)
                .size(11.0)
                .strong()
                .color(egui::Color32::from_rgb(125, 203, 255)),
        );
        ui.add(egui::Separator::default().horizontal());
    });
    ui.add_space(6.0);
}

fn panel_divider(ui: &mut egui::Ui) {
    ui.add_space(10.0);
    let available = ui.available_rect_before_wrap();
    let y = available.top() + 0.5;
    ui.painter().line_segment(
        [
            egui::pos2(available.left(), y),
            egui::pos2(available.right(), y),
        ],
        egui::Stroke::new(1.0, egui::Color32::from_rgb(54, 75, 105)),
    );
    ui.allocate_space(egui::vec2(available.width(), 1.0));
    ui.add_space(2.0);
}

fn pill(ui: &mut egui::Ui, text: &str) {
    egui::Frame::NONE
        .fill(egui::Color32::from_rgb(22, 77, 111))
        .corner_radius(5.0)
        .inner_margin(egui::Margin::symmetric(7, 3))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(text)
                    .size(11.0)
                    .strong()
                    .color(egui::Color32::from_rgb(118, 211, 255)),
            );
        });
}

fn stat(ui: &mut egui::Ui, label: &str, value: String) {
    ui.label(
        egui::RichText::new(label)
            .size(11.0)
            .color(egui::Color32::from_rgb(105, 124, 150)),
    );
    ui.label(
        egui::RichText::new(value)
            .size(11.0)
            .color(egui::Color32::from_rgb(204, 215, 230)),
    );
    ui.end_row();
}

fn labeled_slider(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.set_width(ui.available_width());
        ui.label(
            egui::RichText::new(label)
                .size(11.0)
                .color(egui::Color32::from_rgb(194, 207, 225)),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            changed = ui
                .add(egui::Slider::new(value, range).show_value(true))
                .changed();
        });
    });
    changed
}
