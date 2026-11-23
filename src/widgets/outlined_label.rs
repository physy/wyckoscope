// Adapted from egui-text-outline 0.1.0 (MIT) for Wyckoscope's bevy_egui renderer.

use std::{
    collections::{HashMap, hash_map::DefaultHasher},
    hash::{Hash, Hasher},
    sync::Arc,
};

use bevy::{
    asset::{load_internal_asset, uuid_handle},
    prelude::*,
    render::{
        RenderApp, RenderStartup,
        render_phase::TrackedRenderPass,
        render_resource::{
            AddressMode, BindGroup, BindGroupEntries, BindGroupLayoutDescriptor,
            BindGroupLayoutEntries, BlendState, CachedRenderPipelineId, ColorTargetState,
            ColorWrites, Extent3d, FilterMode, FragmentState, MultisampleState, PipelineCache,
            PrimitiveState, PrimitiveTopology, RenderPipelineDescriptor, Sampler,
            SamplerBindingType, SamplerDescriptor, ShaderStages, SpecializedRenderPipeline,
            SpecializedRenderPipelines, TextureDataOrder, TextureDescriptor, TextureDimension,
            TextureFormat, TextureSampleType, TextureUsages, TextureViewDescriptor, VertexState,
            binding_types::{sampler, texture_2d},
        },
        renderer::{RenderDevice, RenderQueue},
        sync_world::RenderEntity,
    },
    shader::Shader,
};
use bevy_egui::{
    egui::{
        self, Color32, FontId, Galley, Pos2, Rect, Response, Sense, Stroke, Ui, Vec2, Widget,
        epaint::ColorImage,
    },
    render::{EguiBevyPaintCallback, EguiBevyPaintCallbackImpl, EguiPipelineKey},
};

const MAX_MASK_SIDE: usize = 4096;
const MAX_GPU_CACHE_ENTRIES: usize = 512;
const OUTLINED_LABEL_SHADER_HANDLE: Handle<Shader> =
    uuid_handle!("3c486c82-5f7d-4c1b-a727-f9d71a0db316");

/// Installs the dedicated linear-light outline renderer in Bevy's render world.
pub(crate) struct OutlinedLabelPlugin;

impl Plugin for OutlinedLabelPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(
            app,
            OUTLINED_LABEL_SHADER_HANDLE,
            "outlined_label.wgsl",
            Shader::from_wgsl
        );

        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .init_resource::<SpecializedRenderPipelines<OutlinedLabelPipeline>>()
            .init_resource::<PreparedOutlineCache>()
            .add_systems(RenderStartup, init_outlined_label_pipeline);
    }
}

/// An egui label whose glyphs are surrounded by one rasterized outline.
pub(crate) struct OutlinedLabel {
    text: String,
    font_id: Option<FontId>,
    text_color: Option<Color32>,
    outline: Stroke,
}

impl OutlinedLabel {
    pub(crate) fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            font_id: None,
            text_color: None,
            outline: Stroke::NONE,
        }
    }

    #[must_use]
    pub(crate) fn font(mut self, font_id: FontId) -> Self {
        self.font_id = Some(font_id);
        self
    }

    #[must_use]
    pub(crate) fn text_color(mut self, color: Color32) -> Self {
        self.text_color = Some(color);
        self
    }

    #[must_use]
    pub(crate) fn outline(mut self, outline: Stroke) -> Self {
        self.outline = outline;
        self
    }
}

impl Widget for OutlinedLabel {
    fn ui(self, ui: &mut Ui) -> Response {
        let font_id = self
            .font_id
            .unwrap_or_else(|| egui::TextStyle::Body.resolve(ui.style()));
        let text_color = self.text_color.unwrap_or_else(|| ui.visuals().text_color());
        let painter = ui.painter().clone();
        let galley = painter.layout_no_wrap(self.text.clone(), font_id, text_color);

        let pixels_per_point = galley.pixels_per_point;
        let outline_width = if self.outline.width.is_finite() {
            self.outline.width.max(0.0)
        } else {
            0.0
        };
        let radius_pixels = outline_width * pixels_per_point;
        let can_outline = radius_pixels.is_finite()
            && radius_pixels > 0.0
            && radius_pixels < MAX_MASK_SIDE as f32
            && self.outline.color != Color32::TRANSPARENT
            && valid_bounds(galley.mesh_bounds);
        let padding_pixels = if can_outline {
            radius_pixels.ceil() as usize + 1
        } else {
            0
        };
        let padding_points = padding_pixels as f32 / pixels_per_point;
        let content_bounds = if can_outline {
            galley.rect.union(galley.mesh_bounds.expand(padding_points))
        } else {
            galley.rect
        };

        let (rect, response) = ui.allocate_exact_size(content_bounds.size(), Sense::hover());
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Label, ui.is_enabled(), &self.text)
        });
        let galley_pos = rect.min - content_bounds.min.to_vec2();

        if can_outline && ui.is_rect_visible(rect) {
            let cache_id = response.id.with("wyckoscope_outlined_label");
            let radius_key = radius_pixels.to_bits();
            let outline_color = self.outline.color;
            let cached = ui.ctx().data_mut(|data| {
                let state = data.get_temp_mut_or_default::<OutlineState>(cache_id);
                state.galley.as_ref().is_some_and(|cached| {
                    Arc::ptr_eq(cached, &galley)
                        && state.radius_pixels == radius_key
                        && state.outline_color == outline_color
                })
            });

            let (paint_data, image_offset, image_size) = if cached {
                ui.ctx().data_mut(|data| {
                    let state = data.get_temp_mut_or_default::<OutlineState>(cache_id);
                    (
                        state.paint_data.clone(),
                        state.image_offset,
                        state.image_size,
                    )
                })
            } else {
                let font_atlas = painter.fonts(|fonts| fonts.image());
                let max_texture_side = ui
                    .ctx()
                    .input(|input| input.max_texture_side)
                    .min(MAX_MASK_SIDE);
                let outline = make_outline_mask(
                    &galley,
                    &font_atlas,
                    radius_pixels,
                    padding_pixels,
                    max_texture_side,
                );
                let (paint_data, image_offset, image_size) = match outline {
                    Some(outline) => {
                        let paint_data = Arc::new(OutlinePaintData::new(
                            outline.coverage,
                            outline.pixel_size,
                            outline_color,
                        ));
                        (Some(paint_data), outline.offset, outline.size)
                    }
                    None => (None, Vec2::ZERO, Vec2::ZERO),
                };
                ui.ctx().data_mut(|data| {
                    let state = data.get_temp_mut_or_default::<OutlineState>(cache_id);
                    state.galley = Some(Arc::clone(&galley));
                    state.radius_pixels = radius_key;
                    state.outline_color = outline_color;
                    state.paint_data = paint_data.clone();
                    state.image_offset = image_offset;
                    state.image_size = image_size;
                });
                (paint_data, image_offset, image_size)
            };

            if let Some(paint_data) = paint_data {
                let image_rect = Rect::from_min_size(galley_pos + image_offset, image_size);
                painter.add(EguiBevyPaintCallback::new_paint_callback(
                    image_rect,
                    OutlinePaintCallback { paint_data },
                ));
            }
        }

        painter.galley(galley_pos, galley, text_color);
        response
    }
}

#[derive(Clone, Default)]
struct OutlineState {
    galley: Option<Arc<Galley>>,
    radius_pixels: u32,
    outline_color: Color32,
    paint_data: Option<Arc<OutlinePaintData>>,
    image_offset: Vec2,
    image_size: Vec2,
}

struct OutlineMask {
    coverage: Vec<u8>,
    pixel_size: [u32; 2],
    offset: Vec2,
    size: Vec2,
}

struct OutlinePaintData {
    cache_key: u64,
    coverage: Arc<[u8]>,
    pixel_size: [u32; 2],
    color_srgba: [u8; 4],
}

impl OutlinePaintData {
    fn new(coverage: Vec<u8>, pixel_size: [u32; 2], color: Color32) -> Self {
        let color_srgba = color.to_srgba_unmultiplied();
        let mut hasher = DefaultHasher::new();
        pixel_size.hash(&mut hasher);
        color_srgba.hash(&mut hasher);
        coverage.hash(&mut hasher);
        Self {
            cache_key: hasher.finish(),
            coverage: coverage.into(),
            pixel_size,
            color_srgba,
        }
    }
}

struct OutlinePaintCallback {
    paint_data: Arc<OutlinePaintData>,
}

impl EguiBevyPaintCallbackImpl for OutlinePaintCallback {
    fn update(
        &self,
        _info: egui::PaintCallbackInfo,
        _render_entity: RenderEntity,
        pipeline_key: EguiPipelineKey,
        world: &mut World,
    ) {
        let cache_key = (self.paint_data.cache_key, pipeline_key.target_format);
        {
            let mut cache = world.resource_mut::<PreparedOutlineCache>();
            cache.use_counter = cache.use_counter.wrapping_add(1);
            let use_counter = cache.use_counter;
            if let Some(entry) = cache.entries.get_mut(&cache_key) {
                entry.last_used = use_counter;
                return;
            }
        }

        let pipeline_id =
            world.resource_scope(
                |world,
                 mut specialized_pipelines: Mut<
                    SpecializedRenderPipelines<OutlinedLabelPipeline>,
                >| {
                    let pipeline = world.resource::<OutlinedLabelPipeline>();
                    let pipeline_cache = world.resource::<PipelineCache>();
                    specialized_pipelines.specialize(pipeline_cache, pipeline, pipeline_key)
                },
            );
        world
            .resource_mut::<PipelineCache>()
            .block_on_render_pipeline(pipeline_id);

        let pipeline = world.resource::<OutlinedLabelPipeline>();
        let pipeline_cache = world.resource::<PipelineCache>();
        let bind_group_layout = pipeline_cache.get_bind_group_layout(&pipeline.layout);
        let render_device = world.resource::<RenderDevice>();
        let render_queue = world.resource::<RenderQueue>();

        let mask_texture = render_device.create_texture_with_data(
            render_queue,
            &TextureDescriptor {
                label: Some("outlined label coverage mask"),
                size: Extent3d {
                    width: self.paint_data.pixel_size[0],
                    height: self.paint_data.pixel_size[1],
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: TextureDimension::D2,
                format: TextureFormat::R8Unorm,
                usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
                view_formats: &[],
            },
            TextureDataOrder::LayerMajor,
            &self.paint_data.coverage,
        );
        let mask_view = mask_texture.create_view(&TextureViewDescriptor::default());
        let color_texture = render_device.create_texture_with_data(
            render_queue,
            &TextureDescriptor {
                label: Some("outlined label linear color"),
                size: Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: TextureDimension::D2,
                format: TextureFormat::Rgba8UnormSrgb,
                usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
                view_formats: &[],
            },
            TextureDataOrder::LayerMajor,
            &self.paint_data.color_srgba,
        );
        let color_view = color_texture.create_view(&TextureViewDescriptor::default());
        let bind_group = render_device.create_bind_group(
            "outlined label bind group",
            &bind_group_layout,
            &BindGroupEntries::sequential((&mask_view, &pipeline.sampler, &color_view)),
        );

        let mut cache = world.resource_mut::<PreparedOutlineCache>();
        let last_used = cache.use_counter;
        cache.entries.insert(
            cache_key,
            PreparedOutline {
                pipeline_id,
                bind_group,
                last_used,
            },
        );
        cache.evict_oldest_if_needed(cache_key);
    }

    fn render<'pass>(
        &self,
        _info: egui::PaintCallbackInfo,
        render_pass: &mut TrackedRenderPass<'pass>,
        _render_entity: RenderEntity,
        pipeline_key: EguiPipelineKey,
        world: &'pass World,
    ) {
        let cache_key = (self.paint_data.cache_key, pipeline_key.target_format);
        let cache = world.resource::<PreparedOutlineCache>();
        let Some(prepared) = cache.entries.get(&cache_key) else {
            return;
        };
        let pipeline_cache = world.resource::<PipelineCache>();
        let Some(pipeline) = pipeline_cache.get_render_pipeline(prepared.pipeline_id) else {
            return;
        };

        render_pass.set_render_pipeline(pipeline);
        render_pass.set_bind_group(0, &prepared.bind_group, &[]);
        render_pass.draw(0..3, 0..1);
    }
}

#[derive(Resource, Default)]
struct PreparedOutlineCache {
    entries: HashMap<(u64, TextureFormat), PreparedOutline>,
    use_counter: u64,
}

impl PreparedOutlineCache {
    fn evict_oldest_if_needed(&mut self, current: (u64, TextureFormat)) {
        if self.entries.len() <= MAX_GPU_CACHE_ENTRIES {
            return;
        }
        let oldest = self
            .entries
            .iter()
            .filter(|(key, _)| **key != current)
            .min_by_key(|(_, entry)| entry.last_used)
            .map(|(key, _)| *key);
        if let Some(oldest) = oldest {
            self.entries.remove(&oldest);
        }
    }
}

struct PreparedOutline {
    pipeline_id: CachedRenderPipelineId,
    bind_group: BindGroup,
    last_used: u64,
}

#[derive(Resource)]
struct OutlinedLabelPipeline {
    layout: BindGroupLayoutDescriptor,
    sampler: Sampler,
}

fn init_outlined_label_pipeline(mut commands: Commands, render_device: Res<RenderDevice>) {
    let layout = BindGroupLayoutDescriptor::new(
        "outlined label bind group layout",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                texture_2d(TextureSampleType::Float { filterable: true }),
            ),
        ),
    );
    let sampler = render_device.create_sampler(&SamplerDescriptor {
        label: Some("outlined label sampler"),
        address_mode_u: AddressMode::ClampToEdge,
        address_mode_v: AddressMode::ClampToEdge,
        address_mode_w: AddressMode::ClampToEdge,
        mag_filter: FilterMode::Linear,
        min_filter: FilterMode::Linear,
        ..default()
    });
    commands.insert_resource(OutlinedLabelPipeline { layout, sampler });
}

impl SpecializedRenderPipeline for OutlinedLabelPipeline {
    type Key = EguiPipelineKey;

    fn specialize(&self, key: Self::Key) -> RenderPipelineDescriptor {
        RenderPipelineDescriptor {
            label: Some("outlined label pipeline".into()),
            layout: vec![self.layout.clone()],
            immediate_size: 0,
            vertex: VertexState {
                shader: OUTLINED_LABEL_SHADER_HANDLE,
                shader_defs: vec![],
                entry_point: Some("vertex".into()),
                buffers: vec![],
                constants: vec![],
            },
            primitive: PrimitiveState {
                topology: PrimitiveTopology::TriangleList,
                ..default()
            },
            depth_stencil: None,
            multisample: MultisampleState::default(),
            fragment: Some(FragmentState {
                shader: OUTLINED_LABEL_SHADER_HANDLE,
                shader_defs: vec![],
                entry_point: Some("fragment".into()),
                targets: vec![Some(ColorTargetState {
                    format: key.target_format,
                    blend: Some(BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: ColorWrites::ALL,
                })],
                constants: vec![],
            }),
            zero_initialize_workgroup_memory: false,
        }
    }
}

fn valid_bounds(bounds: Rect) -> bool {
    bounds.width().is_finite()
        && bounds.height().is_finite()
        && bounds.width() > 0.0
        && bounds.height() > 0.0
}

fn make_outline_mask(
    galley: &Galley,
    font_atlas: &ColorImage,
    radius_pixels: f32,
    padding_pixels: usize,
    max_texture_side: usize,
) -> Option<OutlineMask> {
    let bounds = galley.mesh_bounds;
    let scale = galley.pixels_per_point;
    let base_width = (bounds.width() * scale).ceil() as usize;
    let base_height = (bounds.height() * scale).ceil() as usize;
    let width = base_width.checked_add(padding_pixels.checked_mul(2)?)?;
    let height = base_height.checked_add(padding_pixels.checked_mul(2)?)?;
    if width == 0 || height == 0 || width > max_texture_side || height > max_texture_side {
        return None;
    }

    let mut glyph_coverage = vec![0_u8; base_width.checked_mul(base_height)?];
    for placed_row in &galley.rows {
        let visuals = &placed_row.row.visuals;
        let mesh = &visuals.mesh;
        let glyph_vertices = &visuals.glyph_vertex_range;
        for triangle in mesh.indices.as_chunks::<3>().0 {
            let indices = [
                triangle[0] as usize,
                triangle[1] as usize,
                triangle[2] as usize,
            ];
            if !indices.iter().all(|index| glyph_vertices.contains(index)) {
                continue;
            }
            let vertices = indices.map(|index| &mesh.vertices[index]);
            let positions = vertices.map(|vertex| {
                let pos = vertex.pos + placed_row.pos.to_vec2();
                Pos2::new(
                    (pos.x - bounds.min.x) * scale,
                    (pos.y - bounds.min.y) * scale,
                )
            });
            rasterize_triangle(
                &mut glyph_coverage,
                base_width,
                base_height,
                positions,
                vertices.map(|vertex| vertex.uv),
                font_atlas,
            );
        }
    }

    let coverage = dilate(
        &glyph_coverage,
        base_width,
        base_height,
        width,
        height,
        padding_pixels,
        radius_pixels,
    );
    Some(OutlineMask {
        coverage,
        pixel_size: [width as u32, height as u32],
        offset: bounds.min.to_vec2() - Vec2::splat(padding_pixels as f32 / scale),
        size: Vec2::new(width as f32 / scale, height as f32 / scale),
    })
}

fn rasterize_triangle(
    coverage: &mut [u8],
    width: usize,
    height: usize,
    positions: [Pos2; 3],
    uvs: [Pos2; 3],
    atlas: &ColorImage,
) {
    let denominator = (positions[1].y - positions[2].y) * (positions[0].x - positions[2].x)
        + (positions[2].x - positions[1].x) * (positions[0].y - positions[2].y);
    if denominator.abs() < f32::EPSILON {
        return;
    }

    let min_x = positions
        .iter()
        .map(|point| point.x)
        .fold(f32::INFINITY, f32::min)
        .floor()
        .max(0.0) as usize;
    let min_y = positions
        .iter()
        .map(|point| point.y)
        .fold(f32::INFINITY, f32::min)
        .floor()
        .max(0.0) as usize;
    let max_x = positions
        .iter()
        .map(|point| point.x)
        .fold(f32::NEG_INFINITY, f32::max)
        .ceil()
        .min(width as f32) as usize;
    let max_y = positions
        .iter()
        .map(|point| point.y)
        .fold(f32::NEG_INFINITY, f32::max)
        .ceil()
        .min(height as f32) as usize;

    for y in min_y..max_y {
        for x in min_x..max_x {
            let point = Pos2::new(x as f32 + 0.5, y as f32 + 0.5);
            let a = ((positions[1].y - positions[2].y) * (point.x - positions[2].x)
                + (positions[2].x - positions[1].x) * (point.y - positions[2].y))
                / denominator;
            let b = ((positions[2].y - positions[0].y) * (point.x - positions[2].x)
                + (positions[0].x - positions[2].x) * (point.y - positions[2].y))
                / denominator;
            let c = 1.0 - a - b;
            if a < 0.0 || b < 0.0 || c < 0.0 {
                continue;
            }

            let uv = Pos2::new(
                a * uvs[0].x + b * uvs[1].x + c * uvs[2].x,
                a * uvs[0].y + b * uvs[1].y + c * uvs[2].y,
            );
            let alpha = sample_alpha(atlas, uv);
            let index = y * width + x;
            coverage[index] = coverage[index].max(alpha);
        }
    }
}

fn sample_alpha(image: &ColorImage, uv: Pos2) -> u8 {
    let x = (uv.x - 0.5).clamp(0.0, image.width().saturating_sub(1) as f32);
    let y = (uv.y - 0.5).clamp(0.0, image.height().saturating_sub(1) as f32);
    let x0 = x.floor() as usize;
    let y0 = y.floor() as usize;
    let x1 = (x0 + 1).min(image.width() - 1);
    let y1 = (y0 + 1).min(image.height() - 1);
    let tx = x - x0 as f32;
    let ty = y - y0 as f32;
    let alpha = |x, y| image[(x, y)].a() as f32;
    let top = alpha(x0, y0) + (alpha(x1, y0) - alpha(x0, y0)) * tx;
    let bottom = alpha(x0, y1) + (alpha(x1, y1) - alpha(x0, y1)) * tx;
    (top + (bottom - top) * ty).round() as u8
}

fn dilate(
    source: &[u8],
    source_width: usize,
    source_height: usize,
    output_width: usize,
    output_height: usize,
    padding: usize,
    radius: f32,
) -> Vec<u8> {
    let mut output = vec![0; output_width * output_height];
    let extent = (radius + 0.5).ceil() as isize;
    for source_y in 0..source_height {
        for source_x in 0..source_width {
            let alpha = source[source_y * source_width + source_x];
            if alpha == 0 {
                continue;
            }
            for dy in -extent..=extent {
                for dx in -extent..=extent {
                    let distance = ((dx * dx + dy * dy) as f32).sqrt();
                    let edge_coverage = (radius + 0.5 - distance).clamp(0.0, 1.0);
                    if edge_coverage == 0.0 {
                        continue;
                    }
                    let output_x = source_x as isize + padding as isize + dx;
                    let output_y = source_y as isize + padding as isize + dy;
                    if output_x < 0
                        || output_y < 0
                        || output_x >= output_width as isize
                        || output_y >= output_height as isize
                    {
                        continue;
                    }
                    let index = output_y as usize * output_width + output_x as usize;
                    let expanded = (alpha as f32 * edge_coverage).round() as u8;
                    output[index] = output[index].max(expanded);
                }
            }
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::{OutlinePaintData, dilate};
    use bevy_egui::egui::Color32;

    #[test]
    fn dilation_expands_a_mask_and_preserves_its_center() {
        let source = [0, 0, 0, 0, 255, 0, 0, 0, 0];
        let output = dilate(&source, 3, 3, 7, 7, 2, 1.0);

        assert_eq!(output[3 * 7 + 3], 255);
        assert!(output[3 * 7 + 4] > 0);
        assert_eq!(output[0], 0);
    }

    #[test]
    fn paint_data_preserves_straight_srgba_color() {
        let color = Color32::from_rgba_unmultiplied(64, 128, 255, 192);
        let data = OutlinePaintData::new(vec![0, 128, 255], [3, 1], color);

        assert_eq!(data.color_srgba, [64, 128, 255, 192]);
        assert_eq!(&*data.coverage, &[0, 128, 255]);
    }
}
