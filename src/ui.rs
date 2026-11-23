use std::{collections::HashSet, sync::Arc};

use crate::{
    AXIS_VIEW_MARGIN, AXIS_VIEW_SIZE, MIN_RENDER_WIDTH, RENDER_TAB_BAR_HEIGHT,
    file_io::{
        request_open_dialog, request_project_open_dialog, request_project_save_dialog,
        request_save_dialog,
    },
    project::{ProjectHistory, ProjectWorkspace, UndoRedoRequests},
    scene::{
        crystal_to_world, default_appearance, measurement_label, periodic_image_count,
        projected_axis_label_centers, scene_object_position, view_directions,
    },
    state::*,
    transport::{TransportAnalysis, analyze as analyze_transport, component_label},
    widgets::OutlinedLabel,
};
use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};

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
    pub const WIDGET_ACTIVE: Color32 = Color32::from_rgb(110, 110, 113);
    pub const SELECTION: Color32 = Color32::from_rgb(144, 144, 158);
}

use palette::*;

const INTERFACE_FONT_NAME: &str = "wyckoscope-noto-sans-jp-regular";
const INTERFACE_BOLD_FONT_NAME: &str = "wyckoscope-noto-sans-jp-bold";
const OVERLAY_LABEL_OUTLINE_WIDTH: f32 = 1.8;
const LEGEND_LABEL_BASE_SIZE: f32 = 18.0;
const LEGEND_SPHERE_BASE_RADIUS: f32 = 14.0;

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(crate) fn interface(
    mut contexts: EguiContexts,
    mut state: ResMut<AppState>,
    mut settings: ResMut<VisualSettings>,
    mut overrides: ResMut<ElementOverrides>,
    mut structure_selection: ResMut<StructureSelection>,
    mut dialogs: ResMut<AsyncDialogs>,
    legend: (ResMut<ExportState>, Res<LegendPreviews>, Res<AxisOverlay>),
    scene_status: Res<SceneStatus>,
    mut selection: ResMut<SelectionState>,
    mut inspector: ResMut<InspectorState>,
    history: (Res<ProjectHistory>, ResMut<ProjectWorkspace>),
    mut history_requests: ResMut<UndoRedoRequests>,
    mut dirty: ResMut<SceneDirty>,
    mut layout: ResMut<InspectorLayout>,
    mut camera: Single<
        (&mut OrbitCamera, &Transform, &Camera, &GlobalTransform),
        With<OrbitCamera>,
    >,
    mut interface_font_registered: Local<bool>,
) -> Result {
    let (mut export, previews, axis_overlay) = legend;
    let (history, mut workspace) = history;
    let ctx = contexts.ctx_mut()?;
    if !*interface_font_registered {
        install_interface_font(ctx);
        *interface_font_registered = true;
        // `Context::set_fonts` is applied at the next egui frame boundary.
        // Do not resolve the named Bold family in this same frame, or egui
        // correctly reports it as not yet bound.
        ctx.request_repaint();
        return Ok(());
    }
    configure_egui_style(ctx);
    let max_panel_width = (ctx.content_rect().width() - MIN_RENDER_WIDTH).clamp(260.0, 560.0);
    let mut viewport_ui = egui::Ui::new(
        ctx.clone(),
        "viewport".into(),
        egui::UiBuilder::new()
            .layer_id(egui::LayerId::background())
            .max_rect(ctx.viewport_rect()),
    );
    let panel = egui::Panel::left("inspector")
        .resizable(true)
        .default_size(layout.width)
        .min_size(260.0)
        .max_size(max_panel_width)
        .frame(
            egui::Frame::NONE
                .fill(panel_background())
                .inner_margin(egui::Margin::same(18)),
        )
        .show(&mut viewport_ui, |ui| {
            app_header(ui);
            ui.add_space(18.0);
            tab_bar(ui, &mut inspector.tab);
            panel_divider(ui);
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| match inspector.tab {
                    InspectorTab::File => ui_file_tab(
                        ui,
                        &mut dialogs,
                        &mut export,
                        &history,
                        &mut history_requests,
                    ),
                    InspectorTab::Structure => ui_structure_tab(
                        ui,
                        &state,
                        &mut settings,
                        &overrides,
                        &scene_status,
                        &mut selection,
                        &mut dirty,
                    ),
                    InspectorTab::Objects => {
                        ui_objects_tab(ui, &state, &mut settings, &mut selection, &mut dirty)
                    }
                    InspectorTab::Vectors => {
                        ui_vectors_tab(ui, &state, &mut settings, &selection, &mut dirty)
                    }
                    InspectorTab::Bonds => {
                        ui_bonds_tab(ui, &state, &mut settings, &overrides, &mut dirty)
                    }
                    InspectorTab::View => {
                        let (orbit, _, _, _) = &mut *camera;
                        ui_view_tab(ui, &state, &mut settings, orbit, &mut dirty);
                    }
                    InspectorTab::Lighting => ui_lighting_tab(ui, &mut settings),
                    InspectorTab::Elements => {
                        ui_elements_tab(ui, &state, &mut settings, &mut overrides, &mut dirty)
                    }
                });
        });
    layout.width = panel.response.rect.width();
    workspace_tabs(ctx, &mut workspace, layout.width);
    let (_, camera_transform, render_camera, camera_global_transform) = &*camera;
    draw_orientation_labels(
        ctx,
        &state,
        &settings,
        camera_transform,
        &axis_overlay,
        layout.width,
    );
    draw_element_legend(ctx, &state, &settings, &overrides, &previews, layout.width);
    draw_scene_object_labels(
        ctx,
        &state,
        &settings,
        render_camera,
        camera_global_transform,
        layout.width,
    );
    structure_selection_dialog(ctx, &mut structure_selection);
    export_preview_modal(
        ctx,
        &state,
        &overrides,
        &axis_overlay,
        &previews,
        camera_transform.rotation,
        &mut settings,
        &mut dialogs,
        &mut export,
    );
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

fn workspace_tabs(ctx: &egui::Context, workspace: &mut ProjectWorkspace, panel_width: f32) {
    let content = ctx.content_rect();
    // The document bar belongs to the render viewport itself. `AXIS_VIEW_MARGIN`
    // only applies to the small orientation gizmo in that viewport.
    let width = (content.width() - panel_width).max(0.0);
    egui::Area::new(egui::Id::new("document_tabs"))
        .order(egui::Order::Foreground)
        .anchor(egui::Align2::LEFT_TOP, egui::vec2(panel_width, 0.0))
        .show(ctx, |ui| {
            // This is a fixed viewport header, rather than a content-sized Frame: paint its
            // entire rectangle so the background never depends on the tab count or scroll width.
            let (bar_rect, _) = ui.allocate_exact_size(
                egui::vec2(width, RENDER_TAB_BAR_HEIGHT),
                egui::Sense::hover(),
            );
            ui.painter()
                .rect_filled(bar_rect, 0.0, WIDGET_INACTIVE_WEAK);
            ui.painter().rect_stroke(
                bar_rect,
                0.0,
                egui::Stroke::new(1.0, CARD_STROKE),
                egui::StrokeKind::Inside,
            );
            ui.scope_builder(
                egui::UiBuilder::new().max_rect(bar_rect.shrink(1.0)),
                |ui| {
                    let scroll = &mut ui.style_mut().spacing.scroll;
                    scroll.bar_width = 4.0;
                    scroll.floating_width = 1.5;
                    scroll.dormant_handle_opacity = 0.18;
                    scroll.active_handle_opacity = 0.4;
                    scroll.interact_handle_opacity = 0.65;
                    egui::ScrollArea::horizontal()
                        .id_salt("document_tab_scroll")
                        .max_height(RENDER_TAB_BAR_HEIGHT)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 0.0;
                                let can_close = workspace.can_close_tabs();
                                for index in 0..workspace.tab_count() {
                                    let Some(label) = workspace.tab_label(index) else {
                                        continue;
                                    };
                                    let label = label.to_owned();
                                    let selected = index == workspace.active_index();
                                    egui::Frame::NONE
                                        .fill(if selected {
                                            WIDGET_ACTIVE
                                        } else {
                                            WIDGET_INACTIVE_WEAK
                                        })
                                        // .inner_margin(egui::Margin::symmetric(5, 1))
                                        .show(ui, |ui| {
                                            ui.add_space(5.0);
                                            if ui
                                                .add(
                                                    egui::Button::new(label)
                                                        .selected(selected)
                                                        .frame(false)
                                                        .min_size(egui::vec2(18.0, 22.0)),
                                                )
                                                .clicked()
                                            {
                                                workspace.request_activate(index);
                                            }
                                            let close = ui.add_enabled(
                                                can_close,
                                                egui::Button::new(
                                                    egui::RichText::new("×").size(14.0),
                                                )
                                                .fill(egui::Color32::TRANSPARENT)
                                                .min_size(egui::vec2(18.0, 22.0)),
                                            );
                                            if close.clicked() {
                                                workspace.request_close(index);
                                            }
                                            if !can_close {
                                                close.on_hover_text("Keep at least one tab open");
                                            }
                                        });
                                }
                            });
                        });
                },
            );
        });
}

fn structure_selection_dialog(ctx: &egui::Context, selection: &mut StructureSelection) {
    if selection.candidates.is_empty() {
        return;
    }
    let mut open = true;
    let mut selected = None;
    egui::Window::new("Choose a CIF data block")
        .collapsible(false)
        .resizable(false)
        .default_width(520.0)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .open(&mut open)
        .show(ctx, |ui| {
            ui.label(
                egui::RichText::new(format!(
                    "{} contains {} complete crystal structures.",
                    selection.file_name,
                    selection.candidates.len()
                ))
                .color(TEXT_PRIMARY),
            );
            ui.label(
                egui::RichText::new("Choose the data block to open.")
                    .size(11.0)
                    .color(TEXT_SECONDARY),
            );
            ui.add_space(8.0);
            for (index, candidate) in selection.candidates.iter().enumerate() {
                let structure = &candidate.structure;
                let name = if candidate.data_name.is_empty() {
                    format!("Data block {}", index + 1)
                } else {
                    candidate.data_name.clone()
                };
                egui::Frame::NONE
                    .fill(CARD_BACKGROUND)
                    .stroke(egui::Stroke::new(1.0, CARD_STROKE))
                    .corner_radius(6.0)
                    .inner_margin(egui::Margin::same(8))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(name).strong().color(ACCENT));
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui.button("Open").clicked() {
                                        selected = Some(index);
                                    }
                                },
                            );
                        });
                        ui.label(
                            egui::RichText::new(format!(
                                "{} · {} sites · a,b,c = {:.3}, {:.3}, {:.3} Å",
                                structure
                                    .space_group
                                    .as_deref()
                                    .unwrap_or("space group not specified"),
                                structure.asymmetric_atoms.len(),
                                structure.cell.a,
                                structure.cell.b,
                                structure.cell.c,
                            ))
                            .size(11.0)
                            .color(TEXT_SECONDARY),
                        );
                    });
                ui.add_space(5.0);
            }
        });
    if let Some(index) = selected {
        selection.selected = Some(index);
    } else if !open {
        selection.candidates.clear();
    }
}

fn draw_orientation_labels(
    ctx: &egui::Context,
    state: &AppState,
    settings: &VisualSettings,
    camera_transform: &Transform,
    axis_overlay: &AxisOverlay,
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
            ui.painter().image(
                axis_overlay.texture_id,
                rect,
                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
            // Labels are separate egui overlays, but their centers come from the same
            // projection/layout function used by preview and PNG export.
            ui.set_clip_rect(ctx.content_rect());
            let axes = state.structure.cell.vectors.map(crystal_to_world);
            let label_positions = projected_axis_label_centers(
                axes,
                camera_transform.rotation,
                AXIS_VIEW_SIZE,
                settings.axis_label_size,
            );
            let font = overlay_label_font(
                egui::FontId::proportional(settings.axis_label_size),
                settings.axis_label_bold,
            );
            for (index, label_position) in label_positions.into_iter().enumerate() {
                let label = ["a", "b", "c"][index];
                let matching_color = egui::Color32::from_rgb(
                    settings.axis_colors[index][0],
                    settings.axis_colors[index][1],
                    settings.axis_colors[index][2],
                );
                let color = overlay_label_color(settings.axis_label_color, matching_color);
                egui::Area::new(egui::Id::new(("orientation_axis_label", index)))
                    .order(egui::Order::Foreground)
                    .fixed_pos(rect.min + egui::vec2(label_position.x, label_position.y))
                    .pivot(egui::Align2::CENTER_CENTER)
                    .interactable(false)
                    .show(ctx, |ui| {
                        outlined_overlay_label(
                            ui,
                            label,
                            font.clone(),
                            color,
                            settings.axis_label_bold,
                        );
                    });
            }
        });
}

fn draw_element_legend(
    ctx: &egui::Context,
    state: &AppState,
    settings: &VisualSettings,
    overrides: &ElementOverrides,
    previews: &LegendPreviews,
    panel_width: f32,
) {
    if !settings.show_elements || !settings.show_element_legend {
        return;
    }
    let mut elements: Vec<_> = state
        .structure
        .visible_atoms(settings.expand_symmetry)
        .iter()
        .filter(|atom| {
            overrides
                .0
                .get(&atom.element)
                .is_none_or(|appearance| appearance.visible)
        })
        .map(|atom| atom.element.clone())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    elements.sort_unstable();
    if elements.is_empty() {
        return;
    }

    egui::Area::new(egui::Id::new("element_legend"))
        .anchor(
            egui::Align2::LEFT_TOP,
            egui::vec2(
                panel_width + AXIS_VIEW_MARGIN,
                RENDER_TAB_BAR_HEIGHT + AXIS_VIEW_MARGIN,
            ),
        )
        .interactable(false)
        .show(ctx, |ui| {
            egui::Frame::NONE
                .inner_margin(egui::Margin::same(2))
                .show(ui, |ui| {
                    let column_count = usize::from(elements.len() > 7) + 1;
                    let rows_per_column = elements.len().div_ceil(column_count);
                    ui.columns(column_count, |columns| {
                        for (column, ui) in columns.iter_mut().enumerate() {
                            let start = column * rows_per_column;
                            let end = (start + rows_per_column).min(elements.len());
                            for symbol in &elements[start..end] {
                                draw_element_legend_item(ui, symbol, overrides, settings, previews);
                            }
                        }
                    });
                });
        });
}

fn draw_scene_object_labels(
    ctx: &egui::Context,
    state: &AppState,
    settings: &VisualSettings,
    camera: &Camera,
    camera_transform: &GlobalTransform,
    panel_width: f32,
) {
    if !settings.show_scene_objects {
        return;
    }
    let supercell = settings.supercell.map(|value| value.max(1));
    let total_vector = state.structure.cell.vectors[0] * supercell[0] as f32
        + state.structure.cell.vectors[1] * supercell[1] as f32
        + state.structure.cell.vectors[2] * supercell[2] as f32;
    let crystal_center = crystal_to_world(total_vector * 0.5);
    let content = ctx.content_rect();
    for object in settings
        .scene_objects
        .objects
        .iter()
        .filter(|object| object.visible)
    {
        let (
            text,
            position,
            font_size,
            show_background,
            background_color,
            background_opacity,
            show_outline,
            outline_color,
            outline_width,
        ) = match &object.kind {
            SceneObjectKind::Label {
                text,
                font_size,
                show_background,
                background_color,
                background_opacity,
                show_outline,
                outline_color,
                outline_width,
            } => (
                text.clone(),
                scene_object_position(object, &state.structure, crystal_center),
                *font_size,
                *show_background,
                *background_color,
                *background_opacity,
                *show_outline,
                *outline_color,
                *outline_width,
            ),
            SceneObjectKind::Measurement {
                kind,
                points,
                label_size,
                show_background,
                background_color,
                background_opacity,
                show_outline,
                outline_color,
                outline_width,
                ..
            } => {
                let Some((text, position)) =
                    measurement_label(*kind, points, &state.structure.cell, crystal_center)
                else {
                    continue;
                };
                (
                    text,
                    position,
                    *label_size,
                    *show_background,
                    *background_color,
                    *background_opacity,
                    *show_outline,
                    *outline_color,
                    *outline_width,
                )
            }
            _ => continue,
        };
        let Ok(viewport) = camera.world_to_viewport_with_depth(camera_transform, position) else {
            continue;
        };
        let screen = egui::pos2(viewport.x, viewport.y);
        if screen.x < panel_width || !content.contains(screen) {
            continue;
        }
        let text_color = egui::Color32::from_rgb(object.color[0], object.color[1], object.color[2]);
        let fill = show_background.then(|| {
            egui::Color32::from_rgba_unmultiplied(
                background_color[0],
                background_color[1],
                background_color[2],
                (background_opacity.clamp(0.0, 1.0) * 255.0).round() as u8,
            )
        });
        let outline = show_outline.then(|| {
            egui::Stroke::new(
                outline_width,
                egui::Color32::from_rgb(outline_color[0], outline_color[1], outline_color[2]),
            )
        });
        egui::Area::new(egui::Id::new(("scene_object_label", object.id)))
            .order(egui::Order::Foreground)
            .fixed_pos(screen)
            .pivot(egui::Align2::CENTER_CENTER)
            .interactable(false)
            .show(ctx, |ui| {
                let mut frame = egui::Frame::NONE;
                if let Some(fill) = fill {
                    frame = frame
                        .fill(fill)
                        .corner_radius(2.0)
                        .inner_margin(egui::Margin::symmetric(4, 2));
                }
                frame.show(ui, |ui| {
                    let mut label = OutlinedLabel::new(text)
                        .font(egui::FontId::proportional(font_size))
                        .text_color(text_color);
                    if let Some(outline) = outline {
                        label = label.outline(outline);
                    }
                    ui.add(label);
                });
            });
    }
}

fn draw_element_legend_item(
    ui: &mut egui::Ui,
    symbol: &str,
    overrides: &ElementOverrides,
    settings: &VisualSettings,
    previews: &LegendPreviews,
) {
    let appearance = overrides
        .0
        .get(symbol)
        .cloned()
        .unwrap_or_else(|| default_appearance(symbol, settings.element_color_scheme));
    let color = egui::Color32::from_rgb(
        appearance.color[0],
        appearance.color[1],
        appearance.color[2],
    );
    let scale = settings.element_legend_scale.clamp(0.5, 3.0);
    let radius = LEGEND_SPHERE_BASE_RADIUS * scale;
    let label_size = LEGEND_LABEL_BASE_SIZE * scale;
    // Keep the sphere's diameter larger than the outlined label at every
    // scale. Native `horizontal` then provides the only alignment needed.
    ui.horizontal(|ui| {
        if let Some(preview) = previews.entries.get(symbol) {
            ui.add(
                egui::Image::new((preview.texture_id, egui::vec2(radius * 2.0, radius * 2.0)))
                    .maintain_aspect_ratio(true),
            );
        } else {
            let (rect, _) = ui
                .allocate_exact_size(egui::vec2(radius * 2.0, radius * 2.0), egui::Sense::hover());
            ui.painter().circle_filled(rect.center(), radius, color);
        }
        outlined_overlay_label(
            ui,
            symbol,
            egui::FontId::proportional(label_size),
            overlay_label_color(settings.element_legend_label_color, color),
            settings.element_legend_label_bold,
        );
    });
}

fn overlay_label_color(mode: OverlayLabelColor, matching_color: egui::Color32) -> egui::Color32 {
    match mode {
        OverlayLabelColor::Black => egui::Color32::BLACK,
        OverlayLabelColor::Matching => matching_color,
    }
}

/// Draw a main-viewport label with a stable white outline and, when selected,
/// the bundled Noto Sans JP Bold face.
fn outlined_overlay_label(
    ui: &mut egui::Ui,
    text: &str,
    font: egui::FontId,
    color: egui::Color32,
    bold: bool,
) -> egui::Response {
    let font = overlay_label_font(font, bold);
    ui.add(
        OutlinedLabel::new(text)
            .font(font.clone())
            .text_color(color)
            .outline(egui::Stroke::new(
                OVERLAY_LABEL_OUTLINE_WIDTH,
                egui::Color32::WHITE,
            )),
    )
}

fn overlay_label_font(mut font: egui::FontId, bold: bool) -> egui::FontId {
    // The Japanese family supplies an actual Bold face.
    if bold && matches!(font.family, egui::FontFamily::Proportional) {
        font.family = egui::FontFamily::Name(INTERFACE_BOLD_FONT_NAME.into());
    }
    font
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
            tab_button(ui, active, InspectorTab::File, "File");
            tab_button(ui, active, InspectorTab::Structure, "Structure");
            ui.end_row();
            tab_button(ui, active, InspectorTab::View, "View");
            tab_button(ui, active, InspectorTab::Elements, "Elements");
            ui.end_row();
            tab_button(ui, active, InspectorTab::Lighting, "Lighting");
            tab_button(ui, active, InspectorTab::Objects, "Objects");
            ui.end_row();
            tab_button(ui, active, InspectorTab::Bonds, "Bonds");
            tab_button(ui, active, InspectorTab::Vectors, "Vectors");
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
fn ui_file_tab(
    ui: &mut egui::Ui,
    dialogs: &mut AsyncDialogs,
    export: &mut ExportState,
    history: &ProjectHistory,
    history_requests: &mut UndoRedoRequests,
) {
    section(ui, "PROJECT", |ui| {
        ui.horizontal(|ui| {
            if ui
                .add_enabled(history.can_undo(), egui::Button::new("Undo"))
                .clicked()
            {
                history_requests.undo = true;
            }
            if ui
                .add_enabled(history.can_redo(), egui::Button::new("Redo"))
                .clicked()
            {
                history_requests.redo = true;
            }
        });
        ui.horizontal(|ui| {
            let project_opening = dialogs.project_open_receiver.is_some();
            if ui
                .add_enabled(!project_opening, egui::Button::new("Open project"))
                .clicked()
            {
                request_project_open_dialog(dialogs);
            }
            let project_saving = dialogs.project_save_receiver.is_some();
            if ui
                .add_enabled(
                    !project_saving,
                    egui::Button::new(if project_saving {
                        "Saving project…"
                    } else {
                        "Save project"
                    }),
                )
                .clicked()
            {
                request_project_save_dialog(dialogs);
            }
        });
    });
    section(ui, "STRUCTURE FILE", |ui| {
        let opening = dialogs.open_receiver.is_some();
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
                        .min_size(egui::vec2(ui.available_width(), 38.0)),
                    )
                },
            )
            .inner;
        if response.clicked() {
            request_open_dialog(dialogs);
        }
        ui.label(
            egui::RichText::new("or drop .cif, .mcif, .vesta, or .wyckoscope anywhere")
                .size(11.0)
                .color(TEXT_SECONDARY),
        );
    });
    ui_export_tab(ui, dialogs, export);
}

fn ui_structure_tab(
    ui: &mut egui::Ui,
    state: &AppState,
    settings: &mut VisualSettings,
    overrides: &ElementOverrides,
    status: &SceneStatus,
    selection: &mut SelectionState,
    dirty: &mut SceneDirty,
) {
    let transport = analyze_transport(&state.structure);
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
    });
    section(ui, "CRYSTALLOGRAPHY", |ui| {
        ui.add_space(12.0);
        egui::Grid::new("crystallography_report")
            .num_columns(2)
            .spacing([18.0, 7.0])
            .show(ui, |ui| {
                let metadata = &state.structure.symmetry_metadata;
                if let Some(parent) = &metadata.parent_space_group_name {
                    stat(ui, "Parent space group", parent.clone());
                    stat(
                        ui,
                        "Parent space group number",
                        metadata.parent_space_group_number.map_or_else(
                            || "not specified".to_string(),
                            |number| number.to_string(),
                        ),
                    );
                    stat(
                        ui,
                        "Magnetic space group (BNS)",
                        metadata
                            .magnetic_space_group_name_bns
                            .clone()
                            .unwrap_or_else(|| "not specified".to_string()),
                    );
                    stat(
                        ui,
                        "Magnetic space group number (BNS)",
                        metadata
                            .magnetic_space_group_number_bns
                            .clone()
                            .unwrap_or_else(|| "not specified".to_string()),
                    );
                    if let Some(transformation) = &metadata.transformation_from_standard {
                        stat(ui, "Transformation from standard", transformation.clone());
                    }
                    if let Some(transformation) = &metadata.transformation_to_standard {
                        stat(ui, "Transformation to standard", transformation.clone());
                    }
                } else {
                    stat(
                        ui,
                        "Lattice centering",
                        metadata
                            .lattice_centering
                            .clone()
                            .unwrap_or_else(|| "not specified".to_string()),
                    );
                    stat(
                        ui,
                        "Space group name",
                        state
                            .structure
                            .space_group
                            .clone()
                            .unwrap_or_else(|| "not specified".to_string()),
                    );
                    stat(
                        ui,
                        "Space group number",
                        metadata.space_group_number.map_or_else(
                            || "not specified".to_string(),
                            |number| number.to_string(),
                        ),
                    );
                    if let Some(setting) = &metadata.setting {
                        stat(ui, "Setting", setting.clone());
                    }
                    if let Some(name) = &metadata.magnetic_space_group_name_bns {
                        stat(ui, "Magnetic space group (BNS)", name.clone());
                        stat(
                            ui,
                            "Magnetic space group number (BNS)",
                            metadata
                                .magnetic_space_group_number_bns
                                .clone()
                                .unwrap_or_else(|| "not specified".to_string()),
                        );
                    }
                }
                stat(
                    ui,
                    "Magnetic point group",
                    transport.magnetic_point_group.clone(),
                );
                stat(
                    ui,
                    "Operations",
                    state.structure.symmetry_operation_count.to_string(),
                );
                stat(
                    ui,
                    "Magnetic point operations",
                    transport.operation_count.to_string(),
                );
                stat(
                    ui,
                    "Volume",
                    format!("{:.2} Å³", state.structure.cell.volume()),
                );
                stat(
                    ui,
                    "a × b × c",
                    format!(
                        "{:.2} × {:.2} × {:.2} Å",
                        state.structure.cell.a, state.structure.cell.b, state.structure.cell.c
                    ),
                );
                stat(
                    ui,
                    "α  β  γ",
                    format!(
                        "{:.0}°  {:.0}°  {:.0}°",
                        state.structure.cell.alpha,
                        state.structure.cell.beta,
                        state.structure.cell.gamma
                    ),
                );
            });
    });
    ui_supercell_section(ui, state, settings, overrides, status, dirty);
    section(ui, "ASYMMETRIC SITES", |ui| {
        egui::ScrollArea::both()
            .max_height(260.0)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                egui::Grid::new("asymmetric_site_report")
                    .striped(true)
                    .spacing([9.0, 5.0])
                    .show(ui, |ui| {
                        for heading in [
                            "No.", "Element", "Label", "x", "y", "z", "Occ.", "Uiso", "Site",
                            "Sym.",
                        ] {
                            ui.label(
                                egui::RichText::new(heading)
                                    .size(11.0)
                                    .strong()
                                    .color(TEXT_SECONDARY),
                            );
                        }
                        ui.end_row();
                        for (index, atom) in state.structure.asymmetric_atoms.iter().enumerate() {
                            for value in [
                                (index + 1).to_string(),
                                atom.element.clone(),
                                atom.label.clone(),
                                format!("{:.5}", atom.fractional_as_written.x),
                                format!("{:.5}", atom.fractional_as_written.y),
                                format!("{:.5}", atom.fractional_as_written.z),
                                format!("{:.3}", atom.occupancy),
                                atom.u_iso
                                    .map_or_else(|| "—".to_string(), |value| format!("{value:.3}")),
                                atom.wyckoff_position
                                    .clone()
                                    .unwrap_or_else(|| "—".to_string()),
                                atom.site_symmetry
                                    .clone()
                                    .unwrap_or_else(|| "—".to_string()),
                            ] {
                                ui.label(egui::RichText::new(value).size(11.0).color(TEXT_PRIMARY));
                            }
                            ui.end_row();
                        }
                    });
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
    ui_transport_analysis(ui, &transport);
    ui_selection_controls(ui, selection, dirty);
    if !state.structure.warnings.is_empty() {
        section(ui, "IMPORT NOTES", |ui| {
            for warning in &state.structure.warnings {
                ui.label(egui::RichText::new(warning).size(11.0).color(WARNING));
            }
        });
    }
}

fn ui_supercell_section(
    ui: &mut egui::Ui,
    state: &AppState,
    settings: &mut VisualSettings,
    overrides: &ElementOverrides,
    status: &SceneStatus,
    dirty: &mut SceneDirty,
) {
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
}

fn ui_selection_controls(
    ui: &mut egui::Ui,
    selection: &mut SelectionState,
    dirty: &mut SceneDirty,
) {
    section(ui, "SELECTION & MEASUREMENT", |ui| {
        ui.label(
            egui::RichText::new(
                "Click an atom to select it. Shift-click adds or removes atoms (up to 4).",
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
        if let [first, second, third, fourth] = selection.atoms.as_slice()
            && let Some(dihedral) = dihedral_degrees(
                first.position,
                second.position,
                third.position,
                fourth.position,
            )
        {
            ui.label(
                egui::RichText::new(format!("Dihedral (1–2–3–4): {dihedral:.3}°"))
                    .size(12.0)
                    .strong()
                    .color(ACCENT),
            );
        }
        if ui.button("Clear selection").clicked() {
            selection.atoms.clear();
            dirty.mark(false);
        }
    });
}

fn dihedral_degrees(first: Vec3, second: Vec3, third: Vec3, fourth: Vec3) -> Option<f32> {
    let first_bond = second - first;
    let middle_bond = third - second;
    let second_bond = fourth - third;
    let normal_a = first_bond.cross(middle_bond).normalize_or_zero();
    let normal_b = middle_bond.cross(second_bond).normalize_or_zero();
    let axis = middle_bond.normalize_or_zero();
    (normal_a.length_squared() > 0.0
        && normal_b.length_squared() > 0.0
        && axis.length_squared() > 0.0)
        .then(|| {
            axis.dot(normal_a.cross(normal_b))
                .atan2(normal_a.dot(normal_b))
                .to_degrees()
        })
}

fn ui_objects_tab(
    ui: &mut egui::Ui,
    state: &AppState,
    settings: &mut VisualSettings,
    selection: &mut SelectionState,
    dirty: &mut SceneDirty,
) {
    section(ui, "SCENE OBJECTS", |ui| {
        if ui
            .checkbox(&mut settings.show_scene_objects, "Show scene objects")
            .changed()
        {
            dirty.mark(false);
        }
        panel_divider(ui);
        ui.horizontal_wrapped(|ui| {
            for (label, kind) in [
                (
                    "Ground",
                    SceneObjectKind::Ground {
                        size: 30.0,
                        metallic: 0.2,
                        roughness: 0.44,
                    },
                ),
                (
                    "Label",
                    SceneObjectKind::Label {
                        text: "Label".to_owned(),
                        font_size: 24.0,
                        show_background: false,
                        background_color: [255, 255, 255],
                        background_opacity: 1.0,
                        show_outline: true,
                        outline_color: [255, 255, 255],
                        outline_width: 2.0,
                    },
                ),
                (
                    "Crystal plane",
                    SceneObjectKind::CrystalPlane {
                        miller: [1, 0, 0],
                        offset: 0.5,
                        size: 8.0,
                        show_normal: true,
                        normal_length: 2.0,
                    },
                ),
                (
                    "Arrow",
                    SceneObjectKind::Arrow {
                        direction: ArrowDirection::Cartesian([0.0, 1.0, 0.0]),
                        shaft_radius: 0.08,
                        head_length: 0.35,
                        head_radius: 0.18,
                    },
                ),
                ("Marker", SceneObjectKind::Marker { radius: 0.25 }),
                ("Sphere", SceneObjectKind::Sphere { radius: 0.5 }),
                (
                    "Box",
                    SceneObjectKind::Box {
                        size: [1.0, 1.0, 1.0],
                    },
                ),
                (
                    "Cylinder",
                    SceneObjectKind::Cylinder {
                        radius: 0.35,
                        height: 1.0,
                    },
                ),
                (
                    "Cone",
                    SceneObjectKind::Cone {
                        radius: 0.45,
                        height: 1.0,
                    },
                ),
                (
                    "Box frame",
                    SceneObjectKind::BoundingBox {
                        size: [2.0, 2.0, 2.0],
                        line_radius: 0.03,
                    },
                ),
                (
                    "Line",
                    SceneObjectKind::Line {
                        start: [-1.0, 0.0, 0.0],
                        end: [1.0, 0.0, 0.0],
                        radius: 0.035,
                    },
                ),
                (
                    "Polyline",
                    SceneObjectKind::Polyline {
                        points: vec![[-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0]],
                        radius: 0.035,
                    },
                ),
            ] {
                if ui.button(format!("+ {label}")).clicked() {
                    if matches!(kind, SceneObjectKind::CrystalPlane { .. }) {
                        settings
                            .scene_objects
                            .add_anchored(kind, SceneObjectAnchor::Crystal);
                    } else {
                        settings.scene_objects.add(kind);
                    }
                    dirty.mark(false);
                }
            }
        });
        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new("Measurements from selected atoms").color(TEXT_SECONDARY));
            for kind in [
                MeasurementKind::Distance,
                MeasurementKind::Angle,
                MeasurementKind::Dihedral,
            ] {
                let enabled = selection.atoms.len() >= kind.point_count();
                if ui
                    .add_enabled(enabled, egui::Button::new(format!("+ {}", kind.label())))
                    .clicked()
                {
                    let points = selection
                        .atoms
                        .iter()
                        .take(kind.point_count())
                        .map(|atom| MeasurementPoint {
                            site_id: atom.site_id,
                            fractional: atom.fractional.to_array(),
                        })
                        .collect();
                    settings.scene_objects.add(SceneObjectKind::Measurement {
                        kind,
                        points,
                        radius: 0.028,
                        label_size: 20.0,
                        show_background: false,
                        background_color: [255, 255, 255],
                        background_opacity: 1.0,
                        show_outline: true,
                        outline_color: [255, 255, 255],
                        outline_width: 5.0,
                    });
                    dirty.mark(false);
                }
            }
        });
        ui.add_space(6.0);
        let mut remove = None;
        let mut duplicate = None;
        let mut changed = false;
        for object in &mut settings.scene_objects.objects {
            egui::CollapsingHeader::new(format!("{} · {}", object.kind.label(), object.name))
                .id_salt(object.id)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        changed |= ui.checkbox(&mut object.visible, "Visible").changed();
                        if ui.button("Duplicate").clicked() {
                            duplicate = Some(object.clone());
                        }
                        if ui.button("Remove").clicked() {
                            remove = Some(object.id);
                        }
                    });
                    changed |= ui.text_edit_singleline(&mut object.name).changed();
                    let supports_position = !matches!(object.kind, SceneObjectKind::Measurement { .. });
                    if supports_position {
                        ui.horizontal(|ui| {
                            ui.label("Anchor");
                            let before = object.anchor;
                            egui::ComboBox::from_id_salt(("anchor", object.id))
                                .selected_text(object.anchor.label())
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(
                                        &mut object.anchor,
                                        SceneObjectAnchor::World,
                                        "World",
                                    );
                                    ui.selectable_value(
                                        &mut object.anchor,
                                        SceneObjectAnchor::Crystal,
                                        "Crystal",
                                    );
                                    ui.separator();
                                    for site in &state.structure.atoms {
                                        let selected = matches!(
                                            object.anchor,
                                            SceneObjectAnchor::Atom { site_id } if site_id == site.site_id
                                        );
                                        if ui
                                            .selectable_label(selected, format!("Atom · {}", site.label))
                                            .clicked()
                                        {
                                            object.anchor = SceneObjectAnchor::Atom {
                                                site_id: site.site_id,
                                            };
                                        }
                                    }
                                });
                            changed |= object.anchor != before;
                        });
                    }
                    let supports_rotation = matches!(
                        object.kind,
                        SceneObjectKind::Ground { .. }
                            | SceneObjectKind::CrystalPlane { .. }
                            | SceneObjectKind::Arrow { .. }
                            | SceneObjectKind::Box { .. }
                            | SceneObjectKind::Cylinder { .. }
                            | SceneObjectKind::Cone { .. }
                            | SceneObjectKind::BoundingBox { .. }
                            | SceneObjectKind::Line { .. }
                            | SceneObjectKind::Polyline { .. }
                    );
                    if supports_position {
                        changed |= vector3_control(ui, "Position", &mut object.position, 0.05);
                    }
                    if supports_rotation {
                        changed |= vector3_control(ui, "Rotation XYZ (°)", &mut object.rotation, 1.0);
                    }
                    ui.horizontal(|ui| {
                        ui.label("Color");
                        changed |= ui.color_edit_button_srgb(&mut object.color).changed();
                    });
                    match &mut object.kind {
                        SceneObjectKind::Ground { size, metallic, roughness } => {
                            changed |= labeled_slider(ui, "Size", size, 1.0..=200.0);
                            changed |= labeled_slider(ui, "Metallic", metallic, 0.0..=1.0);
                            changed |= labeled_slider(ui, "Roughness", roughness, 0.0..=1.0);
                        }
                        SceneObjectKind::Label {
                            text,
                            font_size,
                            show_background,
                            background_color,
                            background_opacity,
                            show_outline,
                            outline_color,
                            outline_width,
                        } => {
                            changed |= ui.text_edit_singleline(text).changed();
                            changed |= labeled_slider(ui, "Font size", font_size, 8.0..=96.0);
                            changed |= label_background_controls(
                                ui,
                                show_background,
                                background_color,
                                background_opacity,
                            );
                            changed |= label_outline_controls(
                                ui,
                                show_outline,
                                outline_color,
                                outline_width,
                            );
                        }
                        SceneObjectKind::CrystalPlane {
                            miller,
                            offset,
                            size,
                            show_normal,
                            normal_length,
                        } => {
                            changed |= integer_vector3_control(ui, "Miller (h k l)", miller);
                            changed |= labeled_slider(ui, "Plane offset", offset, -4.0..=4.0);
                            changed |= labeled_slider(ui, "Size", size, 0.2..=100.0);
                            changed |= ui.checkbox(show_normal, "Show normal").changed();
                            changed |= labeled_slider(ui, "Normal length", normal_length, 0.1..=20.0);
                        }
                        SceneObjectKind::Arrow { direction, shaft_radius, head_length, head_radius } => {
                            ui.horizontal(|ui| {
                                ui.label("Direction source");
                                egui::ComboBox::from_id_salt(("arrow_direction", object.id))
                                    .selected_text(direction.label())
                                    .show_ui(ui, |ui| {
                                        if ui.selectable_label(matches!(direction, ArrowDirection::Cartesian(_)), "Cartesian (x y z)").clicked() {
                                            *direction = ArrowDirection::Cartesian([0.0, 1.0, 0.0]);
                                            changed = true;
                                        }
                                        if ui.selectable_label(matches!(direction, ArrowDirection::CrystalDirection(_)), "Crystal direction [u v w]").clicked() {
                                            *direction = ArrowDirection::CrystalDirection([1, 0, 0]);
                                            changed = true;
                                        }
                                        if ui.selectable_label(matches!(direction, ArrowDirection::PlaneNormal(_)), "Plane normal (h k l)").clicked() {
                                            *direction = ArrowDirection::PlaneNormal([1, 0, 0]);
                                            changed = true;
                                        }
                                    });
                            });
                            match direction {
                                ArrowDirection::Cartesian(values) => {
                                    changed |= vector3_control(ui, "Direction", values, 0.05);
                                }
                                ArrowDirection::CrystalDirection(indices) => {
                                    changed |= integer_vector3_control(ui, "Direction [u v w]", indices);
                                }
                                ArrowDirection::PlaneNormal(indices) => {
                                    changed |= integer_vector3_control(ui, "Plane normal (h k l)", indices);
                                }
                            }
                            changed |= labeled_slider(ui, "Shaft radius", shaft_radius, 0.01..=0.4);
                            changed |= labeled_slider(ui, "Head length", head_length, 0.05..=2.0);
                            changed |= labeled_slider(ui, "Head radius", head_radius, 0.02..=1.0);
                        }
                        SceneObjectKind::Marker { radius } | SceneObjectKind::Sphere { radius } => {
                            changed |= labeled_slider(ui, "Radius", radius, 0.02..=5.0);
                        }
                        SceneObjectKind::Box { size } => {
                            changed |= vector3_control(ui, "Size", size, 0.05);
                        }
                        SceneObjectKind::BoundingBox { size, line_radius } => {
                            changed |= vector3_control(ui, "Size", size, 0.05);
                            changed |= labeled_slider(ui, "Line radius", line_radius, 0.005..=0.2);
                        }
                        SceneObjectKind::Line { start, end, radius } => {
                            changed |= vector3_control(ui, "Start", start, 0.05);
                            changed |= vector3_control(ui, "End", end, 0.05);
                            changed |= labeled_slider(ui, "Radius", radius, 0.002..=0.3);
                        }
                        SceneObjectKind::Polyline { points, radius } => {
                            changed |= labeled_slider(ui, "Radius", radius, 0.002..=0.3);
                            let mut remove_point = None;
                            let can_remove_point = points.len() > 2;
                            for (index, point) in points.iter_mut().enumerate() {
                                ui.horizontal(|ui| {
                                    changed |= vector3_control(ui, &format!("Point {}", index + 1), point, 0.05);
                                    if can_remove_point && ui.small_button("−").clicked() {
                                        remove_point = Some(index);
                                    }
                                });
                            }
                            if let Some(index) = remove_point {
                                points.remove(index);
                                changed = true;
                            }
                            if ui.small_button("+ Point").clicked() {
                                points.push([0.0, 0.0, 0.0]);
                                changed = true;
                            }
                        }
                        SceneObjectKind::Measurement {
                            kind,
                            points,
                            radius,
                            label_size,
                            show_background,
                            background_color,
                            background_opacity,
                            show_outline,
                            outline_color,
                            outline_width,
                        } => {
                            ui.label(format!("{} atom references", points.len()));
                            changed |= labeled_slider(ui, "Line radius", radius, 0.002..=0.3);
                            changed |= labeled_slider(ui, "Label size", label_size, 8.0..=72.0);
                            changed |= label_background_controls(
                                ui,
                                show_background,
                                background_color,
                                background_opacity,
                            );
                            changed |= label_outline_controls(
                                ui,
                                show_outline,
                                outline_color,
                                outline_width,
                            );
                            if points.len() != kind.point_count() {
                                ui.label(egui::RichText::new("Incomplete measurement").color(WARNING));
                            }
                        }
                        SceneObjectKind::Cylinder { radius, height } | SceneObjectKind::Cone { radius, height } => {
                            changed |= labeled_slider(ui, "Radius", radius, 0.02..=5.0);
                            changed |= labeled_slider(ui, "Height", height, 0.02..=10.0);
                        }
                    }
                });
        }
        if let Some(id) = remove {
            settings
                .scene_objects
                .objects
                .retain(|object| object.id != id);
            settings.scene_objects.selected = None;
            changed = true;
        }
        if let Some(mut object) = duplicate {
            object.id = settings.scene_objects.next_id;
            settings.scene_objects.next_id += 1;
            object.name.push_str(" copy");
            settings.scene_objects.selected = Some(object.id);
            settings.scene_objects.objects.push(object);
            changed = true;
        }
        if changed {
            dirty.mark(false);
        }
    });
}

fn ui_vectors_tab(
    ui: &mut egui::Ui,
    state: &AppState,
    settings: &mut VisualSettings,
    selection: &SelectionState,
    dirty: &mut SceneDirty,
) {
    section(ui, "ATOMIC VECTORS / SPINS", |ui| {
        if ui
            .checkbox(&mut settings.show_vectors, "Show all vectors")
            .changed()
        {
            dirty.mark(false);
        }
        panel_divider(ui);
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
            let target = vector_target_description(&vector.target, state);
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

fn vector_target_description(target: &VectorTarget, state: &AppState) -> String {
    let (site_id, individual) = match target {
        VectorTarget::Site { site_id } => (*site_id, false),
        VectorTarget::IndividualAtom { site_id, .. } => (*site_id, true),
    };
    let site = state
        .structure
        .asymmetric_atoms
        .iter()
        .find(|atom| atom.site_id == site_id)
        .map(|atom| format!("{} {}", atom.element, atom.label))
        .unwrap_or_else(|| format!("Missing site #{}", site_id.0));
    if individual {
        format!("{site} (individual)")
    } else {
        site
    }
}

fn ui_bonds_tab(
    ui: &mut egui::Ui,
    state: &AppState,
    settings: &mut VisualSettings,
    overrides: &ElementOverrides,
    dirty: &mut SceneDirty,
) {
    section(ui, "BOND DISPLAY", |ui| {
        if ui
            .checkbox(&mut settings.show_bonds, "Show bonds and polyhedra")
            .changed()
        {
            dirty.mark(false);
        }
        panel_divider(ui);
        ui.label(
            egui::RichText::new(
                "Bonds are drawn only for enabled bond rules; each rule has its own style.",
            )
            .size(11.0)
            .color(TEXT_SECONDARY),
        );
    });

    ui_bond_rule_controls(ui, state, settings, overrides, dirty);

    section(ui, "POLYHEDRA", |ui| {
        let rule_polyhedra = settings
            .bond_rules
            .iter()
            .filter(|rule| rule.enabled && rule.show_polyhedra)
            .count();
        if rule_polyhedra > 0 {
            ui.label(
                egui::RichText::new(format!(
                    "{rule_polyhedra} bond rule(s) also generate polyhedra with A1 as the center."
                ))
                .size(11.0)
                .color(TEXT_SECONDARY),
            );
        }
        if rule_polyhedra > 0 {
            ui.label(
                egui::RichText::new("Color and opacity are configured on each bond rule.")
                    .size(11.0)
                    .color(TEXT_SECONDARY),
            );
            if ui
                .checkbox(&mut settings.polyhedron_show_edges, "Show edges")
                .changed()
            {
                dirty.mark(false);
            }
            if settings.polyhedron_show_edges {
                if labeled_slider(
                    ui,
                    "Edge radius",
                    &mut settings.polyhedron_edge_radius,
                    0.005..=0.12,
                ) {
                    dirty.mark(false);
                }
                ui.horizontal(|ui| {
                    ui.label("Edge color");
                    if ui
                        .color_edit_button_srgb(&mut settings.polyhedron_edge_color)
                        .changed()
                    {
                        dirty.mark(false);
                    }
                });
            }
        }
    });
}

fn ui_transport_analysis(ui: &mut egui::Ui, analysis: &TransportAnalysis) {
    ui.add_space(10.0);
    egui::Frame::NONE
        .fill(CARD_BACKGROUND)
        .stroke(egui::Stroke::new(1.0, CARD_STROKE))
        .corner_radius(5.0)
        .inner_margin(egui::Margin::same(8))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            egui::CollapsingHeader::new(
                egui::RichText::new("CONDUCTIVITY TENSORS")
                    .size(12.0)
                    .strong()
                    .color(TEXT_PRIMARY),
            )
            .id_salt("conductivity_tensors")
            .default_open(false)
            .show(ui, |ui| {
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(
                        "Each cell is written in independent symmetry-allowed parameters. 0 means symmetry-forbidden.",
                    )
                    .size(11.0)
                    .color(TEXT_SECONDARY),
                );
                ui.label(
                    egui::RichText::new(
                        "This is a symmetry-only result; it does not establish a nonzero measured response.",
                    )
                    .size(11.0)
                    .color(TEXT_SECONDARY),
                );
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new("Linear conductivity σ_ij")
                        .strong()
                        .color(ACCENT),
                );
                egui::Grid::new("linear_conductivity_tensor")
                    .num_columns(4)
                    .spacing([10.0, 5.0])
                    .show(ui, |ui| {
                        ui.label("");
                        for axis in ["x", "y", "z"] {
                            ui.label(egui::RichText::new(axis).strong());
                        }
                        ui.end_row();
                        for (row, axis) in ["x", "y", "z"].iter().enumerate() {
                            ui.label(egui::RichText::new(*axis).strong());
                            for column in 0..3 {
                                let index = row * 3 + column;
                                let expression = &analysis.linear.expressions[index];
                                let color = if expression == "0" { TEXT_SECONDARY } else { ACCENT };
                                ui.label(egui::RichText::new(expression).size(12.0).color(color))
                                    .on_hover_text(format!("σ_{}{}", axis, ["x", "y", "z"][column]));
                            }
                            ui.end_row();
                        }
                    });
                ui.add_space(13.0);
                ui.label(
                    egui::RichText::new("Second-order conductivity σ_i;jk")
                        .strong()
                        .color(ACCENT),
                );
                for mechanism in &analysis.mechanisms {
            ui.label(
                egui::RichText::new(mechanism.mechanism.label())
                    .strong()
            );
            egui::Grid::new(egui::Id::new(("transport_tensor", mechanism.mechanism)))
                .num_columns(7)
                .spacing([7.0, 5.0])
                .show(ui, |ui| {
                    ui.label("");
                    for suffix in ["xx", "yy", "zz", "yz", "zx", "xy"] {
                        ui.label(egui::RichText::new(suffix).strong());
                    }
                    ui.end_row();
                    for (output, axis) in ["x", "y", "z"].iter().enumerate() {
                        ui.label(egui::RichText::new(*axis).strong());
                        for pair in [0, 3, 5, 4, 2, 1] {
                            let index = output * 6 + pair;
                            let expression = &mechanism.expressions[index];
                            let color = if expression == "0" {
                                TEXT_SECONDARY
                            } else {
                                ACCENT
                            };
                            ui.label(egui::RichText::new(expression).size(12.0).color(color))
                                .on_hover_text(component_label(index));
                        }
                        ui.end_row();
                    }
                });
            ui.add_space(8.0);
                }
            });
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
        egui::Grid::new("camera_direction_buttons")
            .num_columns(4)
            .spacing(egui::vec2(6.0, 6.0))
            .show(ui, |ui| {
                let mut column = 0;
                for (label, direction) in view_directions(&state.structure) {
                    if ui
                        .add_sized([64.0, 29.0], egui::Button::new(label))
                        .clicked()
                    {
                        orbit.look_from(direction);
                    }
                    column += 1;
                    if label == "c" {
                        if ui
                            .add_sized([64.0, 29.0], egui::Button::new("Default"))
                            .clicked()
                        {
                            orbit.reset();
                        }
                        column += 1;
                    }
                    if column == 4 {
                        ui.end_row();
                        column = 0;
                    }
                }
                if column != 0 {
                    ui.end_row();
                }
            });
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
        overlay_label_style_controls(
            ui,
            "Axis label color",
            &mut settings.axis_label_color,
            &mut settings.axis_label_bold,
        );
    });
    section(ui, "ELEMENT LEGEND", |ui| {
        ui.checkbox(&mut settings.show_element_legend, "Show visible elements");
        overlay_label_style_controls(
            ui,
            "Label color",
            &mut settings.element_legend_label_color,
            &mut settings.element_legend_label_bold,
        );
        labeled_slider(
            ui,
            "Legend size",
            &mut settings.element_legend_scale,
            0.5..=3.0,
        );
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
    section(ui, "APPEARANCE", |ui| {
        if labeled_slider(ui, "Global atom scale", &mut settings.atom_scale, 0.4..=2.0) {
            dirty.mark(false);
        }
        if labeled_slider(ui, "Metallic", &mut settings.atom_metallic, 0.0..=1.0) {
            dirty.mark(false);
        }
        if labeled_slider(ui, "Roughness", &mut settings.atom_roughness, 0.0..=1.0) {
            dirty.mark(false);
        }
        if labeled_slider(ui, "Reflectance", &mut settings.atom_reflectance, 0.0..=1.0) {
            dirty.mark(false);
        }
        if ui
            .checkbox(
                &mut settings.show_thermal_ellipsoids,
                "Thermal ellipsoids (Uij)",
            )
            .changed()
        {
            dirty.mark(false);
        }
        if settings.show_thermal_ellipsoids
            && labeled_slider(
                ui,
                "Ellipsoid scale",
                &mut settings.thermal_ellipsoid_scale,
                0.5..=5.0,
            )
        {
            dirty.mark(false);
        }
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
        ui_unit_cell_controls(ui, settings, dirty);
        egui::Grid::new("view_toggles")
            .num_columns(2)
            .spacing([18.0, 8.0])
            .show(ui, |ui| {
                if ui
                    .checkbox(&mut settings.show_outlines, "Outlines")
                    .changed()
                {
                    dirty.mark(false);
                }
                ui.end_row();
                ui.checkbox(&mut settings.auto_rotate, "Auto rotate");
                ui.label("");
            });
    });
}

fn ui_lighting_tab(ui: &mut egui::Ui, settings: &mut VisualSettings) {
    ui_lighting_controls(
        ui,
        &mut settings.lighting,
        &mut settings.exposure_ev,
        &mut settings.hdr_tonemapping,
    );
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

fn bond_targets(state: &AppState, mode: BondMatchMode) -> Vec<String> {
    let mut targets: Vec<_> = state
        .structure
        .asymmetric_atoms
        .iter()
        .map(|atom| match mode {
            BondMatchMode::Element => atom.element.clone(),
            BondMatchMode::SiteLabel => atom.label.clone(),
        })
        .collect();
    targets.sort();
    targets.dedup();
    targets
}

fn default_polyhedron_color(
    target: &str,
    match_mode: BondMatchMode,
    state: &AppState,
    scheme: wyckoscope::elements::ElementColorScheme,
    overrides: &ElementOverrides,
) -> [u8; 3] {
    let symbol = match match_mode {
        BondMatchMode::Element => target,
        BondMatchMode::SiteLabel => state
            .structure
            .asymmetric_atoms
            .iter()
            .find(|atom| atom.label.eq_ignore_ascii_case(target))
            .map_or(target, |atom| atom.element.as_str()),
    };
    overrides
        .0
        .get(symbol)
        .map(|appearance| appearance.color)
        .unwrap_or_else(|| default_appearance(symbol, scheme).color)
}

fn bond_target_combo(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    value: &mut String,
    targets: &[String],
) -> bool {
    let selected = if value.is_empty() {
        "Select…".to_string()
    } else {
        value.clone()
    };
    let mut changed = false;
    egui::ComboBox::from_id_salt(id)
        .selected_text(selected)
        .show_ui(ui, |ui| {
            for target in targets {
                changed |= ui.selectable_value(value, target.clone(), target).changed();
            }
        });
    changed
}

fn ui_bond_rule_controls(
    ui: &mut egui::Ui,
    state: &AppState,
    settings: &mut VisualSettings,
    overrides: &ElementOverrides,
    dirty: &mut SceneDirty,
) {
    section(ui, "BOND RULES", |ui| {
        let element_color_scheme = settings.element_color_scheme;
        ui.label(
            egui::RichText::new("Each rule defines an A1 center, A2 neighbor, and distance range.")
                .size(11.0)
                .color(TEXT_SECONDARY),
        );
        if ui.button("Add bond rule").clicked() {
            let target = bond_targets(state, BondMatchMode::Element)
                .into_iter()
                .next()
                .unwrap_or_default();
            let polyhedron_color = default_polyhedron_color(
                &target,
                BondMatchMode::Element,
                state,
                element_color_scheme,
                overrides,
            );
            settings.bond_rules.push(BondRule {
                center: target.clone(),
                neighbor: target,
                polyhedron_color,
                ..default()
            });
            dirty.mark(false);
        }
        let mut remove = None;
        for (index, rule) in settings.bond_rules.iter_mut().enumerate() {
            let targets = bond_targets(state, rule.match_mode);
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
                    ui.label(
                        egui::RichText::new("Search mode")
                            .size(11.0)
                            .color(TEXT_SECONDARY),
                    );
                    for mode in [
                        BondSearchMode::A2BondedToA1,
                        BondSearchMode::AtomsBondedToA1,
                    ] {
                        if ui
                            .radio_value(&mut rule.search_mode, mode, mode.label())
                            .changed()
                        {
                            dirty.mark(false);
                        }
                    }
                    ui.horizontal(|ui| {
                        ui.label("Match by");
                        for mode in [BondMatchMode::Element, BondMatchMode::SiteLabel] {
                            if ui
                                .radio_value(&mut rule.match_mode, mode, mode.label())
                                .changed()
                            {
                                dirty.mark(false);
                            }
                        }
                    });
                    ui.label(
                        egui::RichText::new("Boundary mode")
                            .size(11.0)
                            .color(TEXT_SECONDARY),
                    );
                    for mode in [
                        BondBoundaryMode::DoNotSearchBeyond,
                        BondBoundaryMode::SearchAdditionalForA1,
                    ] {
                        if ui
                            .radio_value(&mut rule.boundary_mode, mode, mode.label())
                            .changed()
                        {
                            dirty.mark(false);
                        }
                    }
                    egui::Grid::new(egui::Id::new(("bond_rule", index)))
                        .num_columns(2)
                        .spacing([8.0, 6.0])
                        .show(ui, |ui| {
                            ui.label("Center A1");
                            if bond_target_combo(
                                ui,
                                ("bond_center", index),
                                &mut rule.center,
                                &targets,
                            ) {
                                dirty.mark(false);
                            }
                            ui.end_row();
                            ui.label("Neighbor A2");
                            if rule.search_mode == BondSearchMode::A2BondedToA1 {
                                if bond_target_combo(
                                    ui,
                                    ("bond_neighbor", index),
                                    &mut rule.neighbor,
                                    &targets,
                                ) {
                                    dirty.mark(false);
                                }
                            } else {
                                ui.label("Any atom");
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
                    ui.add_space(5.0);
                    ui.horizontal(|ui| {
                        ui.label("Bond style");
                        egui::ComboBox::from_id_salt(("bond_style", index))
                            .selected_text(rule.bond_style.label())
                            .show_ui(ui, |ui| {
                                for style in BondStyle::ALL {
                                    if ui
                                        .selectable_value(
                                            &mut rule.bond_style,
                                            style,
                                            style.label(),
                                        )
                                        .changed()
                                    {
                                        dirty.mark(false);
                                    }
                                }
                            });
                    });
                    ui.horizontal(|ui| {
                        ui.label("Bond color");
                        if ui.color_edit_button_srgb(&mut rule.bond_color).changed() {
                            dirty.mark(false);
                        }
                    });
                    if labeled_slider(ui, "Bond radius (Å)", &mut rule.bond_radius, 0.005..=0.30) {
                        dirty.mark(false);
                    }
                    if labeled_slider(ui, "Line width (px)", &mut rule.bond_pixel_width, 0.5..=8.0)
                    {
                        dirty.mark(false);
                    }
                    if rule.show_polyhedra {
                        ui.add_space(5.0);
                        ui.horizontal(|ui| {
                            ui.label("Polyhedron color");
                            if ui
                                .color_edit_button_srgb(&mut rule.polyhedron_color)
                                .changed()
                            {
                                dirty.mark(false);
                            }
                            if ui.button("Use A1 color").clicked() {
                                rule.polyhedron_color = default_polyhedron_color(
                                    &rule.center,
                                    rule.match_mode,
                                    state,
                                    element_color_scheme,
                                    overrides,
                                );
                                dirty.mark(false);
                            }
                        });
                        if labeled_slider(
                            ui,
                            "Polyhedron opacity",
                            &mut rule.polyhedron_opacity,
                            0.05..=0.85,
                        ) {
                            dirty.mark(false);
                        }
                    }
                });
            ui.add_space(7.0);
        }
        if let Some(index) = remove {
            settings.bond_rules.remove(index);
            dirty.mark(false);
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
            ui.radio_value(
                &mut lighting.key_kind,
                KeyLightKind::Directional,
                "Directional",
            );
        });
        color_control(ui, "Color", &mut lighting.key_color);
        if lighting.key_kind == KeyLightKind::Directional {
            labeled_slider(
                ui,
                "Illuminance (lux)",
                &mut lighting.key_directional_illuminance,
                0.0..=100_000.0,
            );
        } else {
            labeled_slider(
                ui,
                "Intensity (lm)",
                &mut lighting.key_intensity,
                0.0..=5_000_000.0,
            );
            labeled_slider(ui, "Range", &mut lighting.key_range, 1.0..=500.0);
            labeled_slider(ui, "Source radius", &mut lighting.key_radius, 0.0..=20.0);
        }
        ui.horizontal(|ui| {
            ui.radio_value(&mut lighting.key_mode, LightMode::Camera, "Follow camera");
            ui.radio_value(&mut lighting.key_mode, LightMode::World, "Fixed position");
        });
        match lighting.key_mode {
            LightMode::Camera => {
                if lighting.key_kind != KeyLightKind::Directional {
                    labeled_slider(
                        ui,
                        "Light distance",
                        &mut lighting.key_follow_distance,
                        0.1..=200.0,
                    );
                }
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
                if lighting.key_kind == KeyLightKind::Directional {
                    let _ = vector3_control(
                        ui,
                        "Direction rotation XYZ (°)",
                        &mut lighting.key_directional_rotation,
                        1.0,
                    );
                } else {
                    let _ = vector3_control(ui, "Position", &mut lighting.key_position, 0.1);
                }
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

fn overlay_label_style_controls(
    ui: &mut egui::Ui,
    color_label: &str,
    color_mode: &mut OverlayLabelColor,
    bold: &mut bool,
) {
    ui.label(
        egui::RichText::new(color_label)
            .size(11.0)
            .color(TEXT_SECONDARY),
    );
    ui.horizontal(|ui| {
        for mode in [OverlayLabelColor::Black, OverlayLabelColor::Matching] {
            ui.radio_value(color_mode, mode, mode.label());
        }
    });
    ui.checkbox(bold, "Bold");
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

fn integer_vector3_control(ui: &mut egui::Ui, label: &str, values: &mut [i32; 3]) -> bool {
    ui.label(egui::RichText::new(label).size(11.0).color(TEXT_SECONDARY));
    let mut changed = false;
    ui.horizontal(|ui| {
        for (axis, value) in ["H", "K", "L"].into_iter().zip(values) {
            ui.label(axis);
            changed |= ui.add(egui::DragValue::new(value).speed(1)).changed();
        }
    });
    changed
}

fn ui_elements_tab(
    ui: &mut egui::Ui,
    state: &AppState,
    settings: &mut VisualSettings,
    overrides: &mut ElementOverrides,
    dirty: &mut SceneDirty,
) {
    section(ui, "ELEMENT STYLES", |ui| {
        if ui
            .checkbox(&mut settings.show_elements, "Show all elements")
            .changed()
        {
            dirty.mark(false);
        }
        panel_divider(ui);
        let old_scheme = settings.element_color_scheme;
        ui.horizontal(|ui| {
            ui.label("Color scheme");
            egui::ComboBox::from_id_salt("element_color_scheme")
                .selected_text(settings.element_color_scheme.label())
                .show_ui(ui, |ui| {
                    for scheme in wyckoscope::elements::ElementColorScheme::ALL {
                        ui.selectable_value(
                            &mut settings.element_color_scheme,
                            scheme,
                            scheme.label(),
                        );
                    }
                });
        });
        if settings.element_color_scheme != old_scheme {
            // Older versions materialized a default override as soon as this
            // tab was opened. An appearance whose colour is still that old
            // default should follow the new palette; radius and visibility
            // remain independent customizations.
            for (symbol, appearance) in &mut overrides.0 {
                let old_default = default_appearance(symbol, old_scheme);
                if appearance.color == old_default.color {
                    appearance.color =
                        default_appearance(symbol, settings.element_color_scheme).color;
                }
            }
            // Drop fully-default entries so the selected palette remains the
            // single source of truth for uncustomized elements.
            overrides.0.retain(|symbol, appearance| {
                *appearance != default_appearance(symbol, settings.element_color_scheme)
            });
            dirty.mark(false);
        }
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new(
                "The selected scheme supplies defaults. Individual edits take priority.",
            )
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
            let default = default_appearance(&symbol, settings.element_color_scheme);
            let mut appearance = overrides
                .0
                .get(&symbol)
                .cloned()
                .unwrap_or_else(|| default.clone());
            let mut reset = false;
            let mut changed = false;
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
                                reset = true;
                            }
                            if ui.color_edit_button_srgb(&mut appearance.color).changed() {
                                changed = true;
                            }
                            if ui.checkbox(&mut appearance.visible, "Visible").changed() {
                                changed = true;
                            }
                        });
                    });
                    if labeled_slider(ui, "Radius (Å)", &mut appearance.radius, 0.1..=2.5) {
                        changed = true;
                    }
                });
            if reset {
                if overrides.0.remove(&symbol).is_some() {
                    dirty.mark(false);
                }
            } else if changed {
                overrides.0.insert(symbol.clone(), appearance);
                dirty.mark(false);
            }
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

fn ui_export_tab(ui: &mut egui::Ui, dialogs: &mut AsyncDialogs, export: &mut ExportState) {
    section(ui, "HIGH-RESOLUTION PNG", |ui| {
        ui.label(
            egui::RichText::new("Configure the image and arrange its contents in the preview.")
                .size(11.0)
                .color(TEXT_SECONDARY),
        );
        let busy = dialogs.save_receiver.is_some() || export.stage.is_some() || export.capturing;
        ui.add_space(14.0);
        if ui
            .scope_builder(
                egui::UiBuilder::new().layout(egui::Layout::top_down(egui::Align::Center)),
                |ui| {
                    ui.add_enabled(
                        !busy,
                        egui::Button::new(if busy {
                            "Exporting…"
                        } else {
                            "Configure and export…"
                        })
                        .min_size(egui::vec2(ui.available_width(), 40.0)),
                    )
                },
            )
            .inner
            .clicked()
        {
            export.preview_open = true;
        }
        if let Some(message) = &export.message {
            ui.label(egui::RichText::new(message).size(11.0).color(ACCENT));
        }
    });
}

#[allow(clippy::too_many_arguments)]
fn export_preview_modal(
    ctx: &egui::Context,
    state: &AppState,
    overrides: &ElementOverrides,
    axis_overlay: &AxisOverlay,
    legend_previews: &LegendPreviews,
    camera_rotation: Quat,
    settings: &mut VisualSettings,
    dialogs: &mut AsyncDialogs,
    export: &mut ExportState,
) {
    if !export.preview_open {
        return;
    }
    let mut close = false;
    egui::Modal::new(egui::Id::new("export_preview_modal"))
        .backdrop_color(egui::Color32::from_black_alpha(190))
        .show(ctx, |ui| {
            ui.set_width(760.0);
            ui.heading("Export preview");
            ui.label(
                egui::RichText::new(
                    "Drag the crystal, axis, or legend. Scroll over an item to resize it.",
                )
                .color(TEXT_SECONDARY),
            );
            ui.add_space(8.0);
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
                ui.separator();
                ui.label("Width");
                ui.add(
                    egui::DragValue::new(&mut settings.export_size[0])
                        .range(256..=8192)
                        .suffix(" px"),
                );
                ui.label("Height");
                ui.add(
                    egui::DragValue::new(&mut settings.export_size[1])
                        .range(256..=8192)
                        .suffix(" px"),
                );
            });
            ui.horizontal(|ui| {
                ui.label("Background");
                egui::ComboBox::from_id_salt("export_preview_background")
                    .selected_text(settings.export_background.label())
                    .show_ui(ui, |ui| {
                        for background in ExportBackground::ALL {
                            ui.selectable_value(
                                &mut settings.export_background,
                                background,
                                background.label(),
                            );
                        }
                    });
                if settings.export_background == ExportBackground::SolidColor {
                    ui.label("Color");
                    ui.color_edit_button_srgb(&mut settings.export_background_color);
                }
                ui.add_space(12.0);
                ui.label("Scene scale");
                ui.add(
                    egui::Slider::new(&mut settings.export_view_scale, 1.0..=5.0).show_value(true),
                );
                if ui.button("Reset framing").clicked() {
                    settings.export_view_scale = 1.0;
                    settings.export_layout.crystal_offset = [0.0, 0.0];
                }
            });
            ui.add_space(6.0);
            let aspect =
                settings.export_size[0].max(1) as f32 / settings.export_size[1].max(1) as f32;
            let (workspace, _) =
                ui.allocate_exact_size(egui::vec2(720.0, 540.0), egui::Sense::hover());
            ui.painter().rect_filled(workspace, 4.0, SURFACE_EXTREME);
            let available = workspace.shrink(54.0);
            let (width, height) = if aspect >= available.width() / available.height() {
                (available.width(), available.width() / aspect)
            } else {
                (available.height() * aspect, available.height())
            };
            let rect = egui::Rect::from_center_size(workspace.center(), egui::vec2(width, height));
            if let Some(preview) = &export.preview {
                ui.painter().with_clip_rect(rect).image(
                    preview.texture_id,
                    rect,
                    egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
            } else {
                ui.painter().rect_filled(rect, 0.0, SURFACE_EXTREME);
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "Preparing 3D preview…",
                    egui::FontId::proportional(18.0),
                    TEXT_SECONDARY,
                );
            }
            let mut symbols: Vec<_> = state
                .structure
                .visible_atoms(settings.expand_symmetry)
                .iter()
                .filter(|atom| {
                    overrides
                        .0
                        .get(&atom.element)
                        .is_none_or(|appearance| appearance.visible)
                })
                .map(|atom| atom.element.clone())
                .collect::<HashSet<_>>()
                .into_iter()
                .collect();
            symbols.sort_unstable();
            draw_export_preview_images(ui, rect, settings, axis_overlay, legend_previews, &symbols);
            draw_export_preview_labels(
                ui,
                rect,
                state,
                camera_rotation,
                settings,
                overrides,
                &symbols,
            );
            ui.painter().rect_stroke(
                rect,
                0.0,
                egui::Stroke::new(1.5, TEXT_SECONDARY),
                egui::StrokeKind::Inside,
            );
            ui.painter().text(
                rect.left_top() + egui::vec2(0.0, -2.0),
                egui::Align2::LEFT_BOTTOM,
                format!("{} × {}", settings.export_size[0], settings.export_size[1]),
                egui::FontId::proportional(11.0),
                TEXT_SECONDARY,
            );
            let canvas_response = ui.interact(
                rect,
                egui::Id::new("export_crystal_layout"),
                egui::Sense::click_and_drag(),
            );
            let axis_size = rect.width().min(rect.height()) * 0.22;
            let axis_rect = export_overlay_rect(
                rect,
                &settings.export_layout.axis,
                axis_size * settings.export_layout.axis.scale.clamp(0.5, 3.0),
                0,
            );
            let axis_response = edit_export_overlay(
                ui,
                rect,
                "axis",
                &mut settings.export_layout.axis,
                axis_rect,
            );
            let legend_icon = rect.width().min(rect.height()) * 0.065;
            let legend_columns = usize::from(symbols.len() > 7) + 1;
            let legend_rows = symbols.len().div_ceil(legend_columns).max(1);
            let legend_size = legend_icon * settings.export_layout.legend.scale.clamp(0.5, 3.0);
            let legend_rect =
                export_overlay_rect(rect, &settings.export_layout.legend, legend_size, 0)
                    .union(export_overlay_rect(
                        rect,
                        &settings.export_layout.legend,
                        legend_size,
                        legend_rows.saturating_sub(1),
                    ))
                    .expand2(egui::vec2(legend_size * 2.0 * legend_columns as f32, 0.0));
            let legend_response = edit_export_overlay(
                ui,
                rect,
                "legend",
                &mut settings.export_layout.legend,
                legend_rect,
            );
            if canvas_response.dragged() && !axis_response.dragged() && !legend_response.dragged() {
                let delta = ui.input(|input| input.pointer.delta());
                settings.export_layout.crystal_offset[0] =
                    (settings.export_layout.crystal_offset[0] + delta.x / rect.width())
                        .clamp(-2.0, 2.0);
                settings.export_layout.crystal_offset[1] =
                    (settings.export_layout.crystal_offset[1] + delta.y / rect.height())
                        .clamp(-2.0, 2.0);
            }
            if canvas_response.hovered() && !axis_response.hovered() && !legend_response.hovered() {
                let scroll = ui.input(|input| input.smooth_scroll_delta().y);
                if scroll != 0.0 {
                    settings.export_view_scale =
                        (settings.export_view_scale * (scroll * 0.002).exp()).clamp(1.0, 5.0);
                }
            }
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                ui.checkbox(&mut settings.export_layout.axis.visible, "Axis");
                ui.checkbox(
                    &mut settings.export_layout.axis.labels_visible,
                    "Axis labels",
                );
                ui.checkbox(&mut settings.export_layout.legend.visible, "Legend");
                ui.checkbox(
                    &mut settings.export_layout.legend.labels_visible,
                    "Legend labels",
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Close").clicked() {
                        close = true;
                    }
                    if ui.button("Export PNG").clicked() {
                        request_save_dialog(
                            dialogs,
                            settings.export_size,
                            settings.export_background,
                            settings.export_background_color,
                            settings.export_view_scale,
                            settings.export_layout.clone(),
                        );
                        close = true;
                    }
                });
            });
        });
    export.preview_open = !close;
}

fn edit_export_overlay(
    ui: &mut egui::Ui,
    canvas: egui::Rect,
    id: &'static str,
    layout: &mut ExportOverlayLayout,
    rect: egui::Rect,
) -> egui::Response {
    if !layout.visible {
        return ui.interact(
            egui::Rect::from_min_size(canvas.min, egui::Vec2::ZERO),
            egui::Id::new(("export_overlay_disabled", id)),
            egui::Sense::hover(),
        );
    }
    let response = ui.interact(
        rect,
        egui::Id::new(("export_overlay", id)),
        egui::Sense::drag(),
    );
    if response.dragged() {
        let delta = ui.input(|input| input.pointer.delta());
        layout.position[0] = (layout.position[0] + delta.x / canvas.width()).clamp(0.0, 1.0);
        layout.position[1] = (layout.position[1] + delta.y / canvas.height()).clamp(0.0, 1.0);
    }
    if response.hovered() {
        let scroll = ui.input(|input| input.smooth_scroll_delta().y);
        if scroll != 0.0 {
            layout.scale = (layout.scale * (1.0 + scroll * 0.002)).clamp(0.5, 3.0);
        }
    }
    response
}

fn export_overlay_rect(
    canvas: egui::Rect,
    layout: &ExportOverlayLayout,
    size: f32,
    row: usize,
) -> egui::Rect {
    let size = size.max(1.0).min(canvas.width()).min(canvas.height());
    let center = egui::pos2(
        canvas.left() + layout.position[0] * canvas.width(),
        canvas.top() + layout.position[1] * canvas.height() + row as f32 * size,
    );
    let min = center - egui::Vec2::splat(size * 0.5);
    egui::Rect::from_min_size(min, egui::Vec2::splat(size))
}

fn export_legend_item_rect(
    canvas: egui::Rect,
    layout: &ExportOverlayLayout,
    size: f32,
    index: usize,
    count: usize,
) -> egui::Rect {
    let columns = usize::from(count > 7) + 1;
    let rows = count.div_ceil(columns).max(1);
    let column = index / rows;
    let row = index % rows;
    let mut item_layout = layout.clone();
    item_layout.position[0] += column as f32 * size * 3.0 / canvas.width().max(1.0);
    export_overlay_rect(canvas, &item_layout, size, row)
}

fn draw_export_preview_images(
    ui: &egui::Ui,
    canvas: egui::Rect,
    settings: &VisualSettings,
    axis_overlay: &AxisOverlay,
    legend_previews: &LegendPreviews,
    symbols: &[String],
) {
    let painter = ui.painter();
    let base = canvas.width().min(canvas.height());
    if settings.export_layout.axis.visible {
        let size = base * 0.22 * settings.export_layout.axis.scale.clamp(0.5, 3.0);
        let rect = export_overlay_rect(canvas, &settings.export_layout.axis, size, 0);
        painter.image(
            axis_overlay.texture_id,
            rect,
            egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
    }
    if settings.export_layout.legend.visible {
        let size = base * 0.065 * settings.export_layout.legend.scale.clamp(0.5, 3.0);
        for (index, symbol) in symbols.iter().enumerate() {
            let Some(preview) = legend_previews.entries.get(symbol) else {
                continue;
            };
            let rect = export_legend_item_rect(
                canvas,
                &settings.export_layout.legend,
                size,
                index,
                symbols.len(),
            );
            painter.image(
                preview.texture_id,
                rect,
                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
    }
}

fn draw_export_preview_labels(
    ui: &egui::Ui,
    canvas: egui::Rect,
    state: &AppState,
    camera_rotation: Quat,
    settings: &VisualSettings,
    overrides: &ElementOverrides,
    symbols: &[String],
) {
    let painter = ui.painter();
    let base = canvas.width().min(canvas.height());
    let draw_label = |position: egui::Pos2,
                      anchor: egui::Align2,
                      text: &str,
                      size: f32,
                      color: egui::Color32,
                      bold: bool| {
        let font = overlay_label_font(egui::FontId::proportional(size), bold);
        for offset in [
            egui::vec2(-1.0, 0.0),
            egui::vec2(1.0, 0.0),
            egui::vec2(0.0, -1.0),
            egui::vec2(0.0, 1.0),
        ] {
            painter.text(
                position + offset,
                anchor,
                text,
                font.clone(),
                egui::Color32::WHITE,
            );
        }
        painter.text(position, anchor, text, font, color);
    };
    let axis = &settings.export_layout.axis;
    if axis.visible && axis.labels_visible {
        let size = base * 0.22 * axis.scale.clamp(0.5, 3.0);
        let font_size = (settings.axis_label_size * size / AXIS_VIEW_SIZE).max(8.0);
        let positions = projected_axis_label_centers(
            state.structure.cell.vectors.map(crystal_to_world),
            camera_rotation,
            size,
            font_size,
        );
        let axis_rect = export_overlay_rect(canvas, axis, size, 0);
        for (index, text) in ["a", "b", "c"].into_iter().enumerate() {
            let color = settings.axis_colors[index];
            let color = match settings.axis_label_color {
                OverlayLabelColor::Black => [0, 0, 0],
                OverlayLabelColor::Matching => color,
            };
            let position = positions[index];
            draw_label(
                axis_rect.min + egui::vec2(position.x, position.y),
                egui::Align2::CENTER_CENTER,
                text,
                font_size,
                egui::Color32::from_rgb(color[0], color[1], color[2]),
                settings.axis_label_bold,
            );
        }
    }
    let legend = &settings.export_layout.legend;
    if legend.visible && legend.labels_visible {
        let icon = base * 0.065 * legend.scale.clamp(0.5, 3.0);
        for (index, symbol) in symbols.iter().enumerate() {
            let appearance = overrides
                .0
                .get(symbol)
                .cloned()
                .unwrap_or_else(|| default_appearance(symbol, settings.element_color_scheme));
            let color = egui::Color32::from_rgb(
                if settings.element_legend_label_color == OverlayLabelColor::Black {
                    0
                } else {
                    appearance.color[0]
                },
                if settings.element_legend_label_color == OverlayLabelColor::Black {
                    0
                } else {
                    appearance.color[1]
                },
                if settings.element_legend_label_color == OverlayLabelColor::Black {
                    0
                } else {
                    appearance.color[2]
                },
            );
            let icon_rect = export_legend_item_rect(canvas, legend, icon, index, symbols.len());
            draw_label(
                egui::pos2(icon_rect.right() + icon * 0.05, icon_rect.center().y),
                egui::Align2::LEFT_CENTER,
                symbol,
                (icon * (LEGEND_LABEL_BASE_SIZE / (2.0 * LEGEND_SPHERE_BASE_RADIUS))).max(8.0),
                color,
                settings.element_legend_label_bold,
            );
        }
    }
}

fn configure_egui_style(ctx: &egui::Context) {
    ctx.set_theme(egui::Theme::Dark);
    let mut style = (*ctx.global_style()).clone();
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
    ctx.set_global_style(style);
}

fn install_interface_font(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        INTERFACE_FONT_NAME.to_owned(),
        Arc::new(egui::FontData::from_static(include_bytes!(
            "../assets/fonts/NotoSansJP-Regular.otf"
        ))),
    );
    fonts.font_data.insert(
        INTERFACE_BOLD_FONT_NAME.to_owned(),
        Arc::new(egui::FontData::from_static(include_bytes!(
            "../assets/fonts/NotoSansJP-Bold.otf"
        ))),
    );
    fonts.families.insert(
        egui::FontFamily::Name(INTERFACE_BOLD_FONT_NAME.into()),
        vec![INTERFACE_BOLD_FONT_NAME.to_owned()],
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

fn label_background_controls(
    ui: &mut egui::Ui,
    enabled: &mut bool,
    color: &mut [u8; 3],
    opacity: &mut f32,
) -> bool {
    let mut changed = ui.checkbox(enabled, "Fill label background").changed();
    if *enabled {
        ui.horizontal(|ui| {
            ui.label("Background color");
            changed |= ui.color_edit_button_srgb(color).changed();
        });
        changed |= labeled_slider(ui, "Background opacity", opacity, 0.0..=1.0);
    }
    changed
}

fn label_outline_controls(
    ui: &mut egui::Ui,
    enabled: &mut bool,
    color: &mut [u8; 3],
    width: &mut f32,
) -> bool {
    let mut changed = ui.checkbox(enabled, "Outline").changed();
    if *enabled {
        ui.horizontal(|ui| {
            ui.label("Outline color");
            changed |= ui.color_edit_button_srgb(color).changed();
        });
        changed |= labeled_slider(ui, "Outline width", width, 0.5..=5.0);
    }
    changed
}
