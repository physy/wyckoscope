use ab_glyph::{Font, FontRef, Glyph, OutlinedGlyph, PxScale, PxScaleFactor, point};
use bevy::{
    camera::{CameraOutputMode, Exposure, RenderTarget, ScalingMode, visibility::RenderLayers},
    core_pipeline::tonemapping::{DebandDither, Tonemapping},
    post_process::bloom::Bloom,
    prelude::*,
    render::{
        render_resource::{BlendState, TextureFormat, TextureUsages},
        view::screenshot::{Screenshot, ScreenshotCaptured},
    },
};
use bevy_egui::{EguiTextureHandle, EguiUserTextures};

use crate::{
    scene::{crystal_to_world, default_appearance, projected_axis_label_centers},
    state::*,
};

const EXPORT_COMPOSITE_LAYER: usize = 5;

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(crate) fn process_export(
    mut commands: Commands,
    mut export: ResMut<ExportState>,
    settings: Res<VisualSettings>,
    main_camera: Single<
        (
            &Transform,
            &Projection,
            &Tonemapping,
            &DebandDither,
            &Exposure,
            &OrbitCamera,
        ),
        With<OrbitCamera>,
    >,
    axis_camera: Single<(&Transform, &Projection), With<OrientationAxisCamera>>,
    state: Res<AppState>,
    overrides: Res<ElementOverrides>,
    legend_previews: Res<LegendPreviews>,
    axis_overlay: Res<AxisOverlay>,
    mut images: ResMut<Assets<Image>>,
    mut egui_textures: ResMut<EguiUserTextures>,
) {
    let symbols = visible_element_symbols(&state, &settings, &overrides);
    let preview_aspect =
        settings.export_size[0].max(1) as f32 / settings.export_size[1].max(1) as f32;
    let (preview_width, preview_height) = if preview_aspect >= 1.0 {
        (1024, (1024.0 / preview_aspect).round().max(1.0) as u32)
    } else {
        ((1024.0 * preview_aspect).round().max(1.0) as u32, 1024)
    };
    let preview_fingerprint = format!(
        "{}x{}|{:?}|{:?}|{:?}|{:?}|{:.3}|{:?}|{:?}|{:?}",
        preview_width,
        preview_height,
        settings.export_background,
        settings.background,
        settings.export_background_color,
        settings.export_layout,
        settings.export_view_scale,
        symbols,
        main_camera.0,
        axis_camera.0,
    );
    if export
        .preview
        .as_ref()
        .is_some_and(|preview| preview.fingerprint != preview_fingerprint)
        && let Some(preview) = export.preview.take()
    {
        despawn_preview(&mut commands, &mut images, &mut egui_textures, preview);
    }
    if !export.preview_open {
        if let Some(preview) = export.preview.take() {
            despawn_preview(&mut commands, &mut images, &mut egui_textures, preview);
        }
    } else if export.preview.is_none() {
        let width = preview_width;
        let height = preview_height;
        let image = images.add(Image::new_target_texture(
            width,
            height,
            TextureFormat::Rgba8UnormSrgb,
            None,
        ));
        let (transform, projection, tonemapping, deband_dither, exposure, orbit) = *main_camera;
        let mut transform = *transform;
        let mut projection = projection.clone();
        apply_export_view_scale(&mut projection, settings.export_view_scale);
        apply_export_view_offset(
            &mut transform,
            &projection,
            orbit,
            settings.export_layout.crystal_offset,
            preview_aspect,
        );
        let background = match settings.export_background {
            ExportBackground::Viewport => Color::srgb_u8(
                settings.background[0],
                settings.background[1],
                settings.background[2],
            ),
            ExportBackground::Transparent => Color::NONE,
            ExportBackground::SolidColor => Color::srgb_u8(
                settings.export_background_color[0],
                settings.export_background_color[1],
                settings.export_background_color[2],
            ),
        };
        let camera = commands
            .spawn((
                Camera3d::default(),
                Camera {
                    order: 0,
                    clear_color: ClearColorConfig::Custom(background),
                    ..default()
                },
                RenderTarget::Image(image.clone().into()),
                projection,
                *tonemapping,
                *deband_dither,
                *exposure,
                Bloom::NATURAL,
                Msaa::Sample4,
                transform,
                ExportCamera,
            ))
            .id();
        // Preview overlays are painted directly by egui from the same textures as the
        // viewport. Only the crystal camera renders into this preview target.
        let overlay_cameras = Vec::new();
        let texture_id = egui_textures.add_image(EguiTextureHandle::Strong(image.clone()));
        export.preview = Some(ExportPreview {
            image,
            texture_id,
            camera,
            overlay_cameras,
            fingerprint: preview_fingerprint,
        });
    }
    if export.stage.is_none()
        && !export.capturing
        && let Some(request) = export.request.take()
    {
        let width = request.width.clamp(256, 8192);
        let height = request.height.clamp(256, 8192);
        let mut image =
            Image::new_target_texture(width, height, TextureFormat::Rgba8UnormSrgb, None);
        image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
        let image_handle = images.add(image);
        let (transform, projection, tonemapping, deband_dither, exposure, orbit) = *main_camera;
        let mut transform = *transform;
        let mut export_projection = projection.clone();
        apply_export_view_scale(&mut export_projection, request.view_scale);
        apply_export_view_offset(
            &mut transform,
            &export_projection,
            orbit,
            request.layout.crystal_offset,
            width as f32 / height as f32,
        );
        let background = match request.background {
            ExportBackground::Viewport => Color::srgb_u8(
                settings.background[0],
                settings.background[1],
                settings.background[2],
            ),
            ExportBackground::Transparent => Color::NONE,
            ExportBackground::SolidColor => Color::srgb_u8(
                request.background_color[0],
                request.background_color[1],
                request.background_color[2],
            ),
        };
        let camera = commands
            .spawn((
                Camera3d::default(),
                Camera {
                    order: 0,
                    clear_color: ClearColorConfig::Custom(background),
                    ..default()
                },
                RenderTarget::Image(image_handle.clone().into()),
                export_projection,
                *tonemapping,
                *deband_dither,
                *exposure,
                Bloom::NATURAL,
                Msaa::Sample4,
                transform,
                ExportCamera,
            ))
            .id();
        let overlay_cameras = spawn_overlay_cameras(
            &mut commands,
            image_handle.clone(),
            width,
            height,
            &request.layout,
            &symbols,
            &axis_overlay.image,
            &legend_previews,
        );
        export.stage = Some(ExportStage {
            image: image_handle,
            camera,
            overlay_cameras,
            layout: request.layout,
            legend_colors: symbols
                .iter()
                .map(|symbol| {
                    let color = overrides
                        .0
                        .get(symbol)
                        .cloned()
                        .unwrap_or_else(|| {
                            default_appearance(symbol, settings.element_color_scheme)
                        })
                        .color;
                    match settings.element_legend_label_color {
                        OverlayLabelColor::Black => [0, 0, 0],
                        OverlayLabelColor::Matching => color,
                    }
                })
                .collect(),
            symbols: symbols.clone(),
            axis_colors: match settings.axis_label_color {
                OverlayLabelColor::Black => [[0, 0, 0]; 3],
                OverlayLabelColor::Matching => settings.axis_colors,
            },
            axis_vectors: state.structure.cell.vectors.map(crystal_to_world),
            axis_camera_rotation: axis_camera.0.rotation,
            axis_label_size: settings.axis_label_size,
            axis_label_bold: settings.axis_label_bold,
            legend_label_bold: settings.element_legend_label_bold,
            width,
            height,
            path: request.path,
            frames_remaining: 3,
        });
    }
    let ready = export.stage.as_mut().is_some_and(|stage| {
        if stage.frames_remaining > 0 {
            stage.frames_remaining -= 1;
            false
        } else {
            true
        }
    });
    if ready {
        let stage = export.stage.take().expect("stage exists");
        export.capturing = true;
        export.message = Some("Encoding PNG…".to_string());
        let path = stage.path.clone();
        let image = stage.image.clone();
        let camera = stage.camera;
        let overlay_cameras = stage.overlay_cameras;
        let layout = stage.layout;
        let symbols = stage.symbols;
        let legend_colors = stage.legend_colors;
        let axis_colors = stage.axis_colors;
        let axis_vectors = stage.axis_vectors;
        let axis_camera_rotation = stage.axis_camera_rotation;
        let axis_label_size = stage.axis_label_size;
        let axis_label_bold = stage.axis_label_bold;
        let legend_label_bold = stage.legend_label_bold;
        let width = stage.width;
        let height = stage.height;
        commands.spawn(Screenshot::image(stage.image)).observe(
            move |captured: On<ScreenshotCaptured>,
                  mut commands: Commands,
                  mut images: ResMut<Assets<Image>>,
                  mut export: ResMut<ExportState>| {
                let result = captured
                    .image
                    .clone()
                    .try_into_dynamic()
                    .map_err(|error| error.to_string())
                    .and_then(|image| {
                        let mut image = image.to_rgba8();
                        composite_export_labels(
                            &mut image,
                            width,
                            height,
                            &layout,
                            &symbols,
                            &legend_colors,
                            axis_colors,
                            axis_vectors,
                            axis_camera_rotation,
                            axis_label_size,
                            axis_label_bold,
                            legend_label_bold,
                        );
                        image.save(&path).map_err(|error| error.to_string())
                    });
                commands.entity(camera).despawn();
                for camera in overlay_cameras.iter().copied() {
                    commands.entity(camera).despawn();
                }
                images.remove(&image);
                export.capturing = false;
                export.message = Some(match result {
                    Ok(()) => format!(
                        "Saved {}",
                        path.file_name().and_then(|v| v.to_str()).unwrap_or("PNG")
                    ),
                    Err(error) => format!("Could not save PNG: {error}"),
                });
            },
        );
    }
}

fn apply_export_view_scale(projection: &mut Projection, view_scale: f32) {
    let view_scale = view_scale.clamp(1.0, 2.5);
    match projection {
        Projection::Perspective(projection) => {
            let half_fov = ((projection.fov * 0.5).tan() / view_scale).atan();
            projection.fov = (half_fov * 2.0).clamp(0.02, std::f32::consts::PI - 0.02);
        }
        Projection::Orthographic(projection) => projection.scale /= view_scale,
        Projection::Custom(_) => {}
    }
}

fn apply_export_view_offset(
    transform: &mut Transform,
    projection: &Projection,
    orbit: &OrbitCamera,
    offset: [f32; 2],
    aspect: f32,
) {
    let view_height = match projection {
        Projection::Perspective(projection) => {
            let distance = transform.translation.distance(orbit.target);
            2.0 * distance * (projection.fov * 0.5).tan()
        }
        Projection::Orthographic(projection) => match projection.scaling_mode {
            ScalingMode::FixedVertical { viewport_height } => viewport_height * projection.scale,
            ScalingMode::FixedHorizontal { viewport_width } => {
                viewport_width * projection.scale / aspect.max(0.001)
            }
            ScalingMode::Fixed { height, .. } => height * projection.scale,
            _ => projection.area.height().abs().max(0.001),
        },
        Projection::Custom(_) => orbit.radius,
    };
    let right = transform.rotation * Vec3::X;
    let up = transform.rotation * Vec3::Y;
    transform.translation +=
        -right * offset[0] * view_height * aspect + up * offset[1] * view_height;
}

fn despawn_preview(
    commands: &mut Commands,
    images: &mut Assets<Image>,
    egui_textures: &mut EguiUserTextures,
    preview: ExportPreview,
) {
    commands.entity(preview.camera).despawn();
    for camera in preview.overlay_cameras {
        commands.entity(camera).despawn();
    }
    egui_textures.remove_image(preview.image.id());
    images.remove(&preview.image);
}

fn visible_element_symbols(
    state: &AppState,
    settings: &VisualSettings,
    overrides: &ElementOverrides,
) -> Vec<String> {
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
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();
    symbols.sort_unstable();
    symbols
}

#[derive(Clone, Copy)]
struct OverlayPlacement {
    min: Vec2,
    size: f32,
}

impl OverlayPlacement {
    fn center(self) -> Vec2 {
        self.min + Vec2::splat(self.size * 0.5)
    }
}

fn overlay_placement(
    width: u32,
    height: u32,
    layout: &ExportOverlayLayout,
    size: u32,
    row: usize,
) -> OverlayPlacement {
    let size = size.max(1).min(width).min(height) as f32;
    let center = Vec2::new(
        layout.position[0] * width as f32,
        layout.position[1] * height as f32 + row as f32 * size,
    );
    OverlayPlacement {
        min: center - Vec2::splat(size * 0.5),
        size,
    }
}

fn legend_item_placement(
    width: u32,
    height: u32,
    layout: &ExportOverlayLayout,
    size: u32,
    index: usize,
    count: usize,
) -> OverlayPlacement {
    let columns = usize::from(count > 7) + 1;
    let rows = count.div_ceil(columns).max(1);
    let column = index / rows;
    let row = index % rows;
    let mut item_layout = layout.clone();
    item_layout.position[0] += column as f32 * size as f32 * 3.0 / width.max(1) as f32;
    overlay_placement(width, height, &item_layout, size, row)
}

#[allow(clippy::too_many_arguments)]
fn spawn_overlay_cameras(
    commands: &mut Commands,
    target: Handle<Image>,
    width: u32,
    height: u32,
    layout: &ExportLayout,
    symbols: &[String],
    axis_overlay_image: &Handle<Image>,
    legend_previews: &LegendPreviews,
) -> Vec<Entity> {
    let mut entities = Vec::new();
    let base = width.min(height) as f32;
    if layout.axis.visible {
        let size = (base * 0.22 * layout.axis.scale.clamp(0.5, 3.0)).round() as u32;
        let placement = overlay_placement(width, height, &layout.axis, size, 0);
        let sprite = commands
            .spawn((
                Sprite {
                    image: axis_overlay_image.clone(),
                    custom_size: Some(Vec2::splat(placement.size)),
                    ..default()
                },
                overlay_sprite_transform(width, height, placement),
                RenderLayers::layer(EXPORT_COMPOSITE_LAYER),
                ExportCamera,
            ))
            .id();
        entities.push(sprite);
    }
    if layout.legend.visible {
        let size = (base * 0.065 * layout.legend.scale.clamp(0.5, 3.0)).round() as u32;
        for (index, symbol) in symbols.iter().enumerate() {
            let Some(preview) = legend_previews.entries.get(symbol) else {
                continue;
            };
            let placement =
                legend_item_placement(width, height, &layout.legend, size, index, symbols.len());
            let sprite = commands
                .spawn((
                    Sprite {
                        image: preview._image.clone(),
                        custom_size: Some(Vec2::splat(placement.size)),
                        ..default()
                    },
                    overlay_sprite_transform(width, height, placement),
                    RenderLayers::layer(EXPORT_COMPOSITE_LAYER),
                    ExportCamera,
                ))
                .id();
            entities.push(sprite);
        }
    }
    if !entities.is_empty() {
        let compositor = commands
            .spawn((
                Camera2d,
                Camera {
                    order: 10,
                    clear_color: ClearColorConfig::None,
                    output_mode: CameraOutputMode::Write {
                        blend_state: Some(BlendState::ALPHA_BLENDING),
                        clear_color: ClearColorConfig::None,
                    },
                    ..default()
                },
                RenderTarget::Image(target.into()),
                RenderLayers::layer(EXPORT_COMPOSITE_LAYER),
                ExportCamera,
            ))
            .id();
        entities.push(compositor);
    }
    entities
}

fn overlay_sprite_transform(width: u32, height: u32, placement: OverlayPlacement) -> Transform {
    let center = placement.center();
    Transform::from_xyz(
        center.x - width as f32 * 0.5,
        height as f32 * 0.5 - center.y,
        0.0,
    )
}

#[allow(clippy::too_many_arguments)]
fn composite_export_labels(
    image: &mut image::RgbaImage,
    width: u32,
    height: u32,
    layout: &ExportLayout,
    symbols: &[String],
    legend_colors: &[[u8; 3]],
    axis_colors: [[u8; 3]; 3],
    axis_vectors: [Vec3; 3],
    axis_camera_rotation: Quat,
    axis_label_size: f32,
    axis_label_bold: bool,
    legend_label_bold: bool,
) {
    let base = width.min(height) as f32;
    if layout.axis.visible && layout.axis.labels_visible {
        let size = (base * 0.22 * layout.axis.scale.clamp(0.5, 3.0)).round() as u32;
        let placement = overlay_placement(width, height, &layout.axis, size, 0);
        let size = placement.size;
        let font_size = (axis_label_size * size / 180.0).max(8.0);
        let positions =
            projected_axis_label_centers(axis_vectors, axis_camera_rotation, size, font_size);
        for (index, text) in ["a", "b", "c"].into_iter().enumerate() {
            let position = positions[index] + placement.min;
            draw_centered_outlined_text(
                image,
                text,
                position.x,
                position.y,
                font_size,
                axis_colors[index],
                axis_label_bold,
            );
        }
    }
    if layout.legend.visible && layout.legend.labels_visible {
        let size = (base * 0.065 * layout.legend.scale.clamp(0.5, 3.0)).round() as u32;
        for (index, symbol) in symbols.iter().enumerate() {
            let placement =
                legend_item_placement(width, height, &layout.legend, size, index, symbols.len());
            let x = placement.min.x + placement.size * 1.05;
            let y = placement.center().y;
            let color = legend_colors.get(index).copied().unwrap_or([0, 0, 0]);
            draw_left_centered_outlined_text(
                image,
                symbol,
                x,
                y,
                (placement.size * (18.0 / 28.0)).max(8.0),
                color,
                legend_label_bold,
            );
        }
    }
}

fn draw_left_centered_outlined_text(
    image: &mut image::RgbaImage,
    text: &str,
    x: f32,
    center_y: f32,
    size: f32,
    color: [u8; 3],
    bold: bool,
) {
    let font = export_font(bold);
    draw_outlined_text(
        image,
        text,
        x,
        center_y - export_font_row_height(&font, size) * 0.5,
        size,
        color,
        bold,
    );
}

fn draw_centered_outlined_text(
    image: &mut image::RgbaImage,
    text: &str,
    center_x: f32,
    center_y: f32,
    size: f32,
    color: [u8; 3],
    bold: bool,
) {
    let font = export_font(bold);
    let width = export_text_width(&font, text, size);
    draw_outlined_text(
        image,
        text,
        center_x - width * 0.5,
        center_y - export_font_row_height(&font, size) * 0.5,
        size,
        color,
        bold,
    );
}

fn draw_outlined_text(
    image: &mut image::RgbaImage,
    text: &str,
    x: f32,
    y: f32,
    size: f32,
    color: [u8; 3],
    bold: bool,
) {
    for (dx, dy) in [(-1.5, 0.0), (1.5, 0.0), (0.0, -1.5), (0.0, 1.5)] {
        draw_text(
            image,
            text,
            x + dx,
            y + dy,
            size,
            [255, 255, 255, 255],
            bold,
        );
    }
    draw_text(
        image,
        text,
        x,
        y,
        size,
        [color[0], color[1], color[2], 255],
        bold,
    );
}

fn draw_text(
    image: &mut image::RgbaImage,
    text: &str,
    x: f32,
    y: f32,
    size: f32,
    color: [u8; 4],
    bold: bool,
) {
    let font = export_font(bold);
    let scale_factor = export_font_scale_factor(&font, size);
    let mut cursor = point(x, y + font.ascent_unscaled() * scale_factor);
    let mut previous = None;
    for character in text.chars() {
        let glyph_id = font.glyph_id(character);
        if let Some(previous) = previous {
            cursor.x += font.kern_unscaled(previous, glyph_id) * scale_factor;
        }
        let glyph = Glyph {
            id: glyph_id,
            scale: PxScale::from(0.0),
            position: cursor,
        };
        if let Some(outline) = font.outline(glyph_id) {
            // egui interprets FontId::size as pixels per em. ab_glyph's
            // `outline_glyph` instead interprets PxScale relative to the font's
            // full ascent/descent height, which makes Noto Sans JP noticeably
            // smaller. Constructing OutlinedGlyph with an em-based factor keeps
            // preview and PNG typography on the same scale.
            let outlined = OutlinedGlyph::new(
                glyph,
                outline,
                PxScaleFactor {
                    horizontal: scale_factor,
                    vertical: scale_factor,
                },
            );
            let bounds = outlined.px_bounds();
            outlined.draw(|gx, gy, coverage| {
                let px = bounds.min.x as i32 + gx as i32;
                let py = bounds.min.y as i32 + gy as i32;
                if px < 0 || py < 0 || px >= image.width() as i32 || py >= image.height() as i32 {
                    return;
                }
                let pixel = image.get_pixel_mut(px as u32, py as u32);
                let alpha = coverage * color[3] as f32 / 255.0;
                for channel in 0..3 {
                    pixel[channel] = (color[channel] as f32 * alpha
                        + pixel[channel] as f32 * (1.0 - alpha))
                        .round() as u8;
                }
                pixel[3] =
                    ((alpha + pixel[3] as f32 / 255.0 * (1.0 - alpha)) * 255.0).round() as u8;
            });
        }
        cursor.x += font.h_advance_unscaled(glyph_id) * scale_factor;
        previous = Some(glyph_id);
    }
}

fn export_font(bold: bool) -> FontRef<'static> {
    let bytes: &'static [u8] = if bold {
        include_bytes!("../assets/fonts/NotoSansJP-Bold.otf")
    } else {
        include_bytes!("../assets/fonts/NotoSansJP-Regular.otf")
    };
    FontRef::try_from_slice(bytes).expect("bundled export font is valid")
}

fn export_font_scale_factor(font: &FontRef<'_>, size: f32) -> f32 {
    size / font
        .units_per_em()
        .expect("bundled export font has valid units per em")
}

fn export_font_row_height(font: &FontRef<'_>, size: f32) -> f32 {
    let factor = export_font_scale_factor(font, size);
    (font.ascent_unscaled() - font.descent_unscaled() + font.line_gap_unscaled()) * factor
}

fn export_text_width(font: &FontRef<'_>, text: &str, size: f32) -> f32 {
    let factor = export_font_scale_factor(font, size);
    let mut width = 0.0;
    let mut previous = None;
    for character in text.chars() {
        let glyph_id = font.glyph_id(character);
        if let Some(previous) = previous {
            width += font.kern_unscaled(previous, glyph_id) * factor;
        }
        width += font.h_advance_unscaled(glyph_id) * factor;
        previous = Some(glyph_id);
    }
    width
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_overlay_can_extend_beyond_image() {
        let layout = ExportOverlayLayout {
            position: [1.0, 1.0],
            scale: 1.0,
            ..default()
        };
        let placement = overlay_placement(800, 600, &layout, 160, 2);
        assert_eq!(placement.center().x, 800.0);
        assert!(placement.center().y > 600.0);
    }

    #[test]
    fn export_text_compositing_changes_pixels() {
        let mut image = image::RgbaImage::from_pixel(200, 80, image::Rgba([0, 0, 0, 0]));
        draw_outlined_text(&mut image, "Fe", 10.0, 10.0, 28.0, [255, 0, 0], true);
        assert!(image.pixels().any(|pixel| pixel[3] != 0));
    }
}
