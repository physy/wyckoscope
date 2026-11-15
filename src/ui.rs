use std::{collections::HashSet, sync::Arc};

use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};

use crate::{
    AXIS_VIEW_MARGIN, AXIS_VIEW_SIZE,
    file_io::{request_open_dialog, request_save_dialog},
    scene::{crystal_to_world, default_appearance, periodic_image_count, view_directions},
    state::*,
};

/// Semantic UI colours. Keep presentation colours here instead of scattering
/// raw RGB values through each inspector section.
mod palette {
    use bevy_egui::egui::Color32;

    pub fn panel_background() -> Color32 {
        Color32::from_rgba_unmultiplied(38, 38, 38, 255)
    }
    pub const SURFACE_PANEL: Color32 = Color32::from_rgb(18, 18, 21);
    pub const SURFACE_WINDOW: Color32 = Color32::from_rgb(28, 28, 32);
    pub const SURFACE_EXTREME: Color32 = Color32::from_rgb(12, 12, 14);
    pub const CARD_BACKGROUND: Color32 = Color32::from_rgb(42, 42, 42);
    pub const CARD_STROKE: Color32 = Color32::from_rgb(108, 108, 116);
    pub const DIVIDER: Color32 = Color32::from_rgb(96, 96, 104);
    pub const TEXT_PRIMARY: Color32 = Color32::from_rgb(242, 247, 255);
    pub const TEXT_SECONDARY: Color32 = Color32::from_rgb(207, 220, 238);
    pub const ACCENT: Color32 = Color32::from_rgb(125, 203, 255);
    pub const WARNING: Color32 = Color32::from_rgb(244, 178, 76);
    pub const PILL_BACKGROUND: Color32 = Color32::from_rgb(74, 74, 82);
    pub const WIDGET_INACTIVE: Color32 = Color32::from_rgb(82, 82, 91);
    pub const WIDGET_INACTIVE_WEAK: Color32 = Color32::from_rgb(63, 63, 70);
    pub const WIDGET_HOVERED: Color32 = Color32::from_rgb(106, 106, 117);
    pub const WIDGET_ACTIVE: Color32 = Color32::from_rgb(130, 130, 143);
    pub const SELECTION: Color32 = Color32::from_rgb(144, 144, 158);
}

use palette::*;

const INTERFACE_FONT_NAME: &str = "wyckoscope-noto-sans-cjk-jp";

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
    mut interface_font_registered: Local<bool>,
) -> Result {
    let ctx = contexts.ctx_mut()?;
    if !*interface_font_registered {
        install_interface_font(ctx);
        *interface_font_registered = true;
    }
    configure_egui_style(ctx);
    let panel = egui::SidePanel::left("inspector")
        .resizable(true)
        .default_width(layout.width)
        .min_width(260.0)
        .max_width(560.0)
        .frame(
            egui::Frame::NONE
                .fill(panel_background())
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
                    InspectorTab::Vectors => {
                        ui_vectors_tab(ui, &mut settings, &selection, &mut dirty)
                    }
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
            let mut projected_axes = Vec::with_capacity(3);
            for (index, axis) in axes.into_iter().enumerate() {
                let view_direction = camera_transform.rotation.inverse() * axis.normalize_or_zero();
                let projected = egui::vec2(view_direction.x, -view_direction.y);
                let projected_length = projected.length();
                let direction = if projected_length > 1.0e-4 {
                    projected / projected_length
                } else {
                    egui::vec2(0.0, -1.0)
                };
                let endpoint = origin + projected * (AXIS_VIEW_SIZE / 3.4 * 1.2);
                projected_axes.push((index, direction, endpoint));
            }
            let axis_segments: Vec<_> = projected_axes
                .iter()
                .map(|(_, _, endpoint)| (origin, *endpoint))
                .collect();
            let mut occupied_labels = Vec::with_capacity(3);

            for (index, direction, endpoint) in projected_axes {
                let label = ["a", "b", "c"][index];
                let color = egui::Color32::from_rgb(
                    settings.axis_colors[index][0],
                    settings.axis_colors[index][1],
                    settings.axis_colors[index][2],
                );
                let galley = painter.layout_no_wrap(label.to_owned(), font.clone(), color);
                let label_size = galley.size();
                let perpendicular = egui::vec2(-direction.y, direction.x);
                let gap = settings.axis_label_size * 0.55 + 7.0;
                let candidate_directions = [
                    direction,
                    (direction + perpendicular * 0.75).normalized(),
                    (direction - perpendicular * 0.75).normalized(),
                    perpendicular,
                    -perpendicular,
                    (direction + perpendicular * 1.4).normalized(),
                    (direction - perpendicular * 1.4).normalized(),
                ];
                let (label_position, label_rect) = candidate_directions
                    .into_iter()
                    .enumerate()
                    .map(|(candidate_index, candidate_direction)| {
                        let position = (endpoint
                            + candidate_direction * (gap + candidate_index as f32 * 1.5))
                            .clamp(label_min, label_max);
                        let label_rect = egui::Rect::from_center_size(position, label_size);
                        let hits_axis = axis_segments.iter().any(|&(start, end)| {
                            segment_intersects_rect(start, end, label_rect.expand(3.0))
                        });
                        let hits_origin = label_rect.expand(7.0).contains(origin);
                        let hits_label = occupied_labels
                            .iter()
                            .any(|other: &egui::Rect| other.expand(4.0).intersects(label_rect));
                        let collisions = usize::from(hits_axis)
                            + usize::from(hits_origin)
                            + usize::from(hits_label);
                        (collisions, candidate_index, position, label_rect)
                    })
                    .min_by_key(|(collisions, candidate_index, _, _)| {
                        (*collisions, *candidate_index)
                    })
                    .map(|(_, _, position, label_rect)| (position, label_rect))
                    .expect("axis label candidates are non-empty");
                occupied_labels.push(label_rect);
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
                    .color(ACCENT),
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
            tab_button(ui, active, InspectorTab::Vectors, "Vectors");
            ui.end_row();
            tab_button(ui, active, InspectorTab::View, "View");
            tab_button(ui, active, InspectorTab::Elements, "Elements");
            ui.end_row();
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
    let open_button_width = ui.available_width();
    let response = ui
        .scope_builder(
            egui::UiBuilder::new().layout(egui::Layout::top_down(egui::Align::Center)),
            |ui| {
                ui.add_enabled(
                    !opening,
                    egui::Button::new(if opening {
                        "Opening file dialog…"
                    } else {
                        "Open structure"
                    })
                    .min_size(egui::vec2(open_button_width, 38.0)),
                )
            },
        )
        .inner;
    if response.clicked() {
        request_open_dialog(dialogs);
    }
    ui.label(
        egui::RichText::new("or drop .cif, .mcif, or .vesta anywhere")
            .size(11.0)
            .color(TEXT_SECONDARY),
    );
    section(ui, "STRUCTURE", |ui| {
        ui.label(
            egui::RichText::new(&state.structure.title)
                .size(16.0)
                .strong()
                .color(TEXT_PRIMARY),
        );
        ui.horizontal_wrapped(|ui| {
            pill(ui, state.structure.source_format.label());
            ui.label(
                egui::RichText::new(&state.file_name)
                    .size(11.0)
                    .color(TEXT_SECONDARY),
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
    });
    section(ui, "SYMMETRY", |ui| {
        if ui
            .checkbox(
                &mut settings.expand_symmetry,
                "Expand atoms with space-group operations",
            )
            .changed()
        {
            dirty.mark(true);
        }
    });
    section(ui, "SUPERCELL", |ui| {
        ui.horizontal(|ui| {
            for (axis, value) in ["A", "B", "C"].into_iter().zip(&mut settings.supercell) {
                ui.vertical(|ui| {
                    ui.label(egui::RichText::new(axis).size(11.0).color(TEXT_SECONDARY));
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
        let supercell = settings.supercell.map(|value| value.max(1));
        let count = state
            .structure
            .visible_atoms(settings.expand_symmetry)
            .iter()
            .filter(|atom| {
                overrides
                    .0
                    .get(&atom.element)
                    .is_none_or(|appearance| appearance.visible)
            })
            .map(|atom| {
                if settings.show_boundary_atoms {
                    periodic_image_count(atom.fractional, supercell)
                } else {
                    supercell
                        .iter()
                        .fold(1_u64, |total, value| total.saturating_mul(*value as u64))
                }
            })
            .sum::<u64>();
        ui.label(
            egui::RichText::new(format!("{count} displayed atoms"))
                .size(11.0)
                .color(TEXT_SECONDARY),
        );
        if let Some(warning) = &status.warning {
            ui.colored_label(WARNING, warning);
        }
    });
    ui_selection_controls(ui, selection, dirty);
    if !state.structure.warnings.is_empty() {
        section(ui, "IMPORT NOTES", |ui| {
            for warning in &state.structure.warnings {
                ui.label(egui::RichText::new(warning).size(11.0).color(WARNING));
            }
        });
    }
}

fn ui_selection_controls(
    ui: &mut egui::Ui,
    selection: &mut SelectionState,
    dirty: &mut SceneDirty,
) {
    section(ui, "SELECTION & MEASUREMENT", |ui| {
        ui.label(
            egui::RichText::new(
                "Click an atom to select it. Shift-click adds or removes atoms (up to 3).",
            )
            .size(11.0)
            .color(TEXT_SECONDARY),
        );
        if selection.atoms.is_empty() {
            ui.label(
                egui::RichText::new("Select an atom in the 3D view to inspect its coordination.")
                    .size(11.0)
                    .color(TEXT_SECONDARY),
            );
            return;
        }
        ui.add_space(5.0);
        for (index, atom) in selection.atoms.iter().enumerate() {
            egui::Frame::NONE
                .fill(CARD_BACKGROUND)
                .stroke(egui::Stroke::new(1.0, CARD_STROKE))
                .corner_radius(6.0)
                .inner_margin(egui::Margin::symmetric(8, 6))
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.label(
                        egui::RichText::new(format!(
                            "{}  {} ({})",
                            index + 1,
                            atom.element,
                            atom.label
                        ))
                        .size(12.0)
                        .strong()
                        .color(ACCENT),
                    );
                    ui.label(
                        egui::RichText::new(format!(
                            "Fractional: {:.4}, {:.4}, {:.4} · coordination: {}",
                            atom.fractional.x,
                            atom.fractional.y,
                            atom.fractional.z,
                            atom.coordination
                        ))
                        .size(11.0)
                        .color(TEXT_SECONDARY),
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
                .color(ACCENT),
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
                    .color(ACCENT),
                );
            }
        }
        if ui.button("Clear selection").clicked() {
            selection.atoms.clear();
            dirty.mark(false);
        }
    });
}

fn ui_vectors_tab(
    ui: &mut egui::Ui,
    settings: &mut VisualSettings,
    selection: &SelectionState,
    dirty: &mut SceneDirty,
) {
    section(ui, "ATOMIC VECTORS / SPINS", |ui| {
        let vector_count = settings.vectors.layers.len();
        ui.label(
            egui::RichText::new(
                "Axial vectors represent magnetic moments. Site vectors follow spatial symmetry.",
            )
            .size(11.0)
            .color(TEXT_SECONDARY),
        );
        ui.add_space(4.0);
        ui.columns(2, |columns| {
            columns[0].label(
                egui::RichText::new("TARGET")
                    .size(11.0)
                    .strong()
                    .color(ACCENT),
            );
            if let Some(atom) = selection.atoms.first() {
                columns[0].label(
                    egui::RichText::new(format!("{} {}", atom.element, atom.label))
                        .size(12.0)
                        .color(TEXT_PRIMARY),
                );
                columns[0].label(
                    egui::RichText::new("Selected in the 3D view")
                        .size(11.0)
                        .color(TEXT_SECONDARY),
                );
            } else {
                columns[0].label(
                    egui::RichText::new("Select an atom in the 3D view")
                        .size(11.0)
                        .color(TEXT_SECONDARY),
                );
            }

            columns[1].label(
                egui::RichText::new("VECTOR DEFINITIONS")
                    .size(11.0)
                    .strong()
                    .color(ACCENT),
            );
            columns[1].label(
                egui::RichText::new(format!(
                    "{vector_count} vector{}",
                    if vector_count == 1 { "" } else { "s" }
                ))
                .size(12.0)
                .color(TEXT_PRIMARY),
            );
        });
        panel_divider(ui);
        if labeled_slider(
            ui,
            "Vector display scale",
            &mut settings.vectors.display_scale,
            0.05..=10.0,
        ) {
            dirty.mark(false);
        }
        if let Some(atom) = selection.atoms.first() {
            ui.horizontal_wrapped(|ui| {
                if ui
                    .button(format!("Add site vector: {}", atom.label))
                    .clicked()
                {
                    settings.vectors.layers.push(VectorLayer::for_site(atom));
                    dirty.mark(false);
                }
                if ui
                    .button(format!("Add selected atom: {}", atom.label))
                    .clicked()
                {
                    settings.vectors.layers.push(VectorLayer::for_atom(atom));
                    dirty.mark(false);
                }
            });
        } else {
            ui.label(
                egui::RichText::new(
                    "Select an atom first to add a site-wide or individual vector.",
                )
                .size(11.0)
                .color(TEXT_SECONDARY),
            );
        }
        let mut remove = None;
        for (index, vector) in settings.vectors.layers.iter_mut().enumerate() {
            let target = match &vector.target {
                VectorTarget::Site { label, element } => format!("{element} {label}"),
                VectorTarget::IndividualAtom { label, element, .. } => {
                    format!("{element} {label}")
                }
            };
            egui::Frame::NONE
                .fill(CARD_BACKGROUND)
                .stroke(egui::Stroke::new(1.0, CARD_STROKE))
                .corner_radius(8.0)
                .inner_margin(egui::Margin::same(9))
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.horizontal(|ui| {
                        if ui.checkbox(&mut vector.enabled, "").changed() {
                            dirty.mark(false);
                        }
                        ui.label(
                            egui::RichText::new(format!("{} · {target}", vector.target.label(),))
                                .size(12.0)
                                .strong()
                                .color(ACCENT),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("Remove").clicked() {
                                remove = Some(index);
                            }
                        });
                    });
                    ui.horizontal(|ui| {
                        ui.label("Type");
                        let before = vector.kind;
                        egui::ComboBox::from_id_salt(("vector_kind", index))
                            .selected_text(vector.kind.label())
                            .show_ui(ui, |ui| {
                                ui.selectable_value(
                                    &mut vector.kind,
                                    VectorKind::Axial,
                                    VectorKind::Axial.label(),
                                );
                                ui.selectable_value(
                                    &mut vector.kind,
                                    VectorKind::Polar,
                                    VectorKind::Polar.label(),
                                );
                            });
                        if vector.kind != before {
                            dirty.mark(false);
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label("Coordinates");
                        let before = vector.coordinates;
                        egui::ComboBox::from_id_salt(("vector_coordinates", index))
                            .selected_text(vector.coordinates.label())
                            .show_ui(ui, |ui| {
                                ui.selectable_value(
                                    &mut vector.coordinates,
                                    VectorCoordinates::Lattice,
                                    VectorCoordinates::Lattice.label(),
                                );
                                ui.selectable_value(
                                    &mut vector.coordinates,
                                    VectorCoordinates::Cartesian,
                                    VectorCoordinates::Cartesian.label(),
                                );
                            });
                        if vector.coordinates != before {
                            dirty.mark(false);
                        }
                    });
                    if vector3_control(ui, "Components", &mut vector.components, 0.05) {
                        dirty.mark(false);
                    }
                    if labeled_slider(ui, "Length", &mut vector.style.scale, 0.05..=5.0) {
                        dirty.mark(false);
                    }
                    if labeled_slider(
                        ui,
                        "Shaft width",
                        &mut vector.style.shaft_radius,
                        0.01..=0.30,
                    ) {
                        dirty.mark(false);
                    }
                    if labeled_slider(ui, "Head length", &mut vector.style.head_length, 0.15..=3.0)
                    {
                        dirty.mark(false);
                    }
                    if labeled_slider(ui, "Head width", &mut vector.style.head_radius, 0.15..=3.0) {
                        dirty.mark(false);
                    }
                    ui.horizontal(|ui| {
                        ui.label("Color");
                        if ui.color_edit_button_srgb(&mut vector.style.color).changed() {
                            dirty.mark(false);
                        }
                        if ui
                            .checkbox(&mut vector.style.penetrate_atom, "Penetrate atom")
                            .changed()
                        {
                            dirty.mark(false);
                        }
                    });
                    if ui
                        .checkbox(&mut vector.style.add_atom_radius, "Offset by atom radius")
                        .changed()
                    {
                        dirty.mark(false);
                    }
                });
            ui.add_space(7.0);
        }
        if let Some(index) = remove {
            settings.vectors.layers.remove(index);
            dirty.mark(false);
        }
    });
}

fn ui_view_tab(
    ui: &mut egui::Ui,
    state: &AppState,
    settings: &mut VisualSettings,
    orbit: &mut OrbitCamera,
    dirty: &mut SceneDirty,
) {
    section(ui, "CAMERA DIRECTION", |ui| {
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
            egui::RichText::new("Left drag: rotate · Shift/right drag: move · Wheel: zoom")
                .size(11.0)
                .color(TEXT_SECONDARY),
        );
    });
    section(ui, "PROJECTION", |ui| {
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
    });
    section(ui, "ORIENTATION AXES", |ui| {
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
                .color(TEXT_SECONDARY),
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
    });
    section(ui, "BACKGROUND", |ui| {
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
    });
    ui_lighting_controls(
        ui,
        &mut settings.lighting,
        &mut settings.exposure_ev,
        &mut settings.hdr_tonemapping,
    );
    section(ui, "APPEARANCE", |ui| {
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
    });
}

fn ui_unit_cell_controls(ui: &mut egui::Ui, settings: &mut VisualSettings, dirty: &mut SceneDirty) {
    section(ui, "UNIT CELL", |ui| {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(egui::RichText::new("Line").size(11.0).color(TEXT_SECONDARY));
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
                        .color(TEXT_SECONDARY),
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
    });
}

fn ui_bond_rule_controls(ui: &mut egui::Ui, settings: &mut VisualSettings, dirty: &mut SceneDirty) {
    section(ui, "BOND RULES & POLYHEDRA", |ui| {
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
        .color(TEXT_SECONDARY),
    );
        if ui.button("Add bond rule").clicked() {
            settings.bond_rules.push(BondRule::default());
            dirty.mark(false);
        }
        let mut remove = None;
        for (index, rule) in settings.bond_rules.iter_mut().enumerate() {
            egui::Frame::NONE
                .fill(CARD_BACKGROUND)
                .stroke(egui::Stroke::new(1.0, CARD_STROKE))
                .corner_radius(8.0)
                .inner_margin(egui::Margin::same(9))
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
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
    });
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
        *exposure_ev = VisualSettings::default().exposure_ev;
        *hdr_tonemapping = VisualSettings::default().hdr_tonemapping;
    }

    section(ui, "AMBIENT LIGHT", |ui| {
        ui.checkbox(&mut lighting.ambient_enabled, "Enabled");
        color_control(ui, "Color", &mut lighting.ambient_color);
        labeled_slider(
            ui,
            "Brightness (cd/m²)",
            &mut lighting.ambient_brightness,
            0.0..=5_000.0,
        );
    });

    section(ui, "KEY LIGHT", |ui| {
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
                lighting.spot_inner_angle =
                    lighting.spot_inner_angle.min(lighting.spot_outer_angle);
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
    });

    section(ui, "FILL LIGHT", |ui| {
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
    });

    section(ui, "AXIS LIGHTING", |ui| {
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
            "Key rotation around camera (°)",
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
            "Fill rotation around camera (°)",
            &mut lighting.axis_fill_rotation,
            1.0,
        );
    });

    section(ui, "CAMERA EXPOSURE", |ui| {
        labeled_slider(ui, "Exposure EV", exposure_ev, 4.0..=13.0);
        ui.checkbox(hdr_tonemapping, "HDR tonemapping");
    });
}

fn color_control(ui: &mut egui::Ui, label: &str, color: &mut [u8; 3]) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.color_edit_button_srgb(color);
    });
}

fn vector3_control(ui: &mut egui::Ui, label: &str, values: &mut [f32; 3], speed: f64) -> bool {
    ui.label(egui::RichText::new(label).size(11.0).color(TEXT_SECONDARY));
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
    section(ui, "ELEMENT STYLES", |ui| {
        ui.label(
            egui::RichText::new("Set the color and visual radius for each element.")
                .size(11.0)
                .color(TEXT_SECONDARY),
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
                .fill(CARD_BACKGROUND)
                .stroke(egui::Stroke::new(1.0, CARD_STROKE))
                .corner_radius(8.0)
                .inner_margin(egui::Margin::same(10))
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.horizontal(|ui| {
                        let title_color = if appearance.visible {
                            TEXT_PRIMARY
                        } else {
                            TEXT_SECONDARY
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
    });
}

fn ui_export_tab(
    ui: &mut egui::Ui,
    settings: &mut VisualSettings,
    dialogs: &mut AsyncDialogs,
    export: &ExportState,
) {
    section(ui, "HIGH-RESOLUTION PNG", |ui| {
        ui.label(
            egui::RichText::new("Exports the 3D view without the inspector UI.")
                .size(11.0)
                .color(TEXT_SECONDARY),
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
        let export_button_width = ui.available_width();
        let response = ui
            .scope_builder(
                egui::UiBuilder::new().layout(egui::Layout::top_down(egui::Align::Center)),
                |ui| {
                    ui.add_enabled(
                        !busy,
                        egui::Button::new(if busy { "Exporting…" } else { "Export PNG" })
                            .min_size(egui::vec2(export_button_width, 40.0)),
                    )
                },
            )
            .inner;
        if response.clicked() {
            request_save_dialog(dialogs, settings.export_size);
        }
        if let Some(message) = &export.message {
            ui.label(egui::RichText::new(message).size(11.0).color(ACCENT));
        }
    });
}

fn configure_egui_style(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    for font_id in style.text_styles.values_mut() {
        font_id.family = interface_font_family();
    }
    style.spacing.item_spacing = egui::vec2(9.0, 8.0);
    style.visuals.dark_mode = true;
    style.visuals.override_text_color = Some(egui::Color32::WHITE);
    style.visuals.panel_fill = SURFACE_PANEL;
    style.visuals.window_fill = SURFACE_WINDOW;
    style.visuals.extreme_bg_color = SURFACE_EXTREME;
    style.visuals.widgets.noninteractive.fg_stroke.color = TEXT_PRIMARY;
    style.visuals.widgets.inactive.bg_fill = WIDGET_INACTIVE;
    style.visuals.widgets.inactive.weak_bg_fill = WIDGET_INACTIVE_WEAK;
    style.visuals.widgets.inactive.fg_stroke.color = TEXT_PRIMARY;
    style.visuals.widgets.hovered.bg_fill = WIDGET_HOVERED;
    style.visuals.widgets.hovered.fg_stroke.color = egui::Color32::WHITE;
    style.visuals.widgets.active.bg_fill = WIDGET_ACTIVE;
    style.visuals.widgets.active.fg_stroke.color = egui::Color32::WHITE;
    style.visuals.selection.bg_fill = SELECTION;
    style.visuals.widgets.inactive.corner_radius = 7.0.into();
    style.visuals.widgets.hovered.corner_radius = 7.0.into();
    style.visuals.widgets.active.corner_radius = 7.0.into();
    ctx.set_style(style);
}

fn install_interface_font(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        INTERFACE_FONT_NAME.to_owned(),
        Arc::new(egui::FontData::from_static(include_bytes!(
            "../assets/fonts/NotoSansCJKjp-Regular.otf"
        ))),
    );
    let mut fallbacks = fonts
        .families
        .get(&egui::FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    fallbacks.insert(0, INTERFACE_FONT_NAME.to_owned());
    fonts
        .families
        .insert(egui::FontFamily::Proportional, fallbacks);
    ctx.set_fonts(fonts);
}

fn interface_font_family() -> egui::FontFamily {
    // Keep this a built-in family: `set_fonts` is applied at the next egui
    // frame boundary, while the current frame may already resolve text.
    // The custom font is installed first in this family's fallback list.
    egui::FontFamily::Proportional
}

fn segment_intersects_rect(start: egui::Pos2, end: egui::Pos2, rect: egui::Rect) -> bool {
    if rect.contains(start) || rect.contains(end) {
        return true;
    }
    let corners = [
        rect.left_top(),
        rect.right_top(),
        rect.right_bottom(),
        rect.left_bottom(),
    ];
    corners
        .into_iter()
        .zip(corners.into_iter().cycle().skip(1))
        .take(4)
        .any(|(edge_start, edge_end)| segments_intersect(start, end, edge_start, edge_end))
}

fn segments_intersect(
    first_start: egui::Pos2,
    first_end: egui::Pos2,
    second_start: egui::Pos2,
    second_end: egui::Pos2,
) -> bool {
    let cross = |left: egui::Vec2, right: egui::Vec2| left.x * right.y - left.y * right.x;
    let first = first_end - first_start;
    let second = second_end - second_start;
    let denominator = cross(first, second);
    if denominator.abs() < 1.0e-6 {
        return false;
    }
    let delta = second_start - first_start;
    let first_t = cross(delta, second) / denominator;
    let second_t = cross(delta, first) / denominator;
    (0.0..=1.0).contains(&first_t) && (0.0..=1.0).contains(&second_t)
}

fn section(ui: &mut egui::Ui, title: &str, contents: impl FnOnce(&mut egui::Ui)) {
    ui.add_space(10.0);
    egui::Frame::NONE
        .fill(CARD_BACKGROUND)
        .stroke(egui::Stroke::new(1.0, CARD_STROKE))
        .corner_radius(5.0)
        .inner_margin(egui::Margin::same(8))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.label(
                egui::RichText::new(title)
                    .size(12.0)
                    .strong()
                    .color(TEXT_PRIMARY),
            );
            ui.add_space(8.0);
            contents(ui);
        });
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
        egui::Stroke::new(1.0, DIVIDER),
    );
    ui.allocate_space(egui::vec2(available.width(), 1.0));
    ui.add_space(2.0);
}

fn pill(ui: &mut egui::Ui, text: &str) {
    egui::Frame::NONE
        .fill(PILL_BACKGROUND)
        .corner_radius(5.0)
        .inner_margin(egui::Margin::symmetric(7, 3))
        .show(ui, |ui| {
            ui.label(egui::RichText::new(text).size(11.0).strong().color(ACCENT));
        });
}

fn stat(ui: &mut egui::Ui, label: &str, value: String) {
    ui.label(egui::RichText::new(label).size(11.0).color(TEXT_SECONDARY));
    ui.label(egui::RichText::new(value).size(11.0).color(TEXT_PRIMARY));
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
        ui.label(egui::RichText::new(label).size(11.0).color(TEXT_SECONDARY));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            changed = ui
                .add(egui::Slider::new(value, range).show_value(true))
                .changed();
        });
    });
    changed
}

#[cfg(test)]
mod tests {
    use super::segment_intersects_rect;
    use bevy_egui::egui;

    #[test]
    fn axis_segment_collision_detects_a_label_rect() {
        let rect = egui::Rect::from_min_max(egui::pos2(4.0, 4.0), egui::pos2(8.0, 8.0));
        assert!(segment_intersects_rect(
            egui::pos2(0.0, 0.0),
            egui::pos2(10.0, 10.0),
            rect
        ));
        assert!(!segment_intersects_rect(
            egui::pos2(0.0, 0.0),
            egui::pos2(3.0, 3.0),
            rect
        ));
    }
}
