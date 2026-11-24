use std::collections::{HashMap, HashSet};

use bevy::{
    asset::RenderAssetUsages,
    camera::{CameraOutputMode, Exposure, RenderTarget, ScalingMode, visibility::RenderLayers},
    core_pipeline::tonemapping::Tonemapping,
    gizmos::{
        GizmoAsset,
        config::{GizmoConfigGroup, GizmoConfigStore, GizmoLineConfig, GizmoLineStyle},
        prelude::Gizmos,
        retained::Gizmo,
    },
    mesh::{Indices, PrimitiveTopology},
    pbr::wireframe::{Wireframe, WireframeColor},
    prelude::*,
    render::render_resource::{BlendState, Face, TextureFormat},
};
use bevy_egui::{EguiTextureHandle, EguiUserTextures, PrimaryEguiContext};
use wyckoscope::{
    elements::{self, ElementStyle},
    model::{AtomSite, CrystalStructure, UnitCell},
};

use crate::{AXIS_RENDER_LAYER, MAX_RENDER_ATOMS, camera::update_camera_transform, state::*};

const BOUNDARY_EPSILON: f32 = 1.0e-4;
pub(crate) const LEGEND_RENDER_LAYER: usize = 2;
const LEGEND_ICON_SIZE: u32 = 96;
const AXIS_OVERLAY_SIZE: u32 = 512;

/// Dedicated screen-space line configuration for the crystallographic cell.
/// Bevy expands these lines after projection, so their width is in pixels.
#[derive(Default, Reflect, GizmoConfigGroup)]
#[reflect(Default)]
pub(crate) struct UnitCellGizmoGroup;

/// Marker for the custom meshes used to visualize partial or mixed occupancy.
/// Unlike a plain sphere, a sector has an observable orientation.
#[derive(Component)]
pub(crate) struct OccupancySector;

/// Project crystallographic axis tips into a square axis overlay and place labels
/// just beyond the rendered arrowheads. Used by both preview and PNG export.
pub(crate) fn projected_axis_label_centers(
    axes: [Vec3; 3],
    camera_rotation: Quat,
    overlay_size: f32,
    label_size: f32,
) -> [Vec2; 3] {
    let origin = Vec2::splat(overlay_size * 0.5);
    let projected = axes.map(|axis| {
        let view = camera_rotation.inverse() * axis.normalize_or_zero();
        let projected = Vec2::new(view.x, -view.y);
        let direction = projected.try_normalize().unwrap_or(Vec2::NEG_Y);
        let endpoint = origin + projected * (overlay_size / 3.4 * 1.2);
        (direction, endpoint)
    });
    let mut centers = [Vec2::ZERO; 3];
    let half_extent = label_size * 0.65;
    let lower = Vec2::splat(half_extent);
    let upper = Vec2::splat((overlay_size - half_extent).max(half_extent));
    for (index, (direction, endpoint)) in projected.into_iter().enumerate() {
        let perpendicular = Vec2::new(-direction.y, direction.x);
        let candidates = [
            direction,
            (direction + perpendicular * 0.75).normalize_or_zero(),
            (direction - perpendicular * 0.75).normalize_or_zero(),
            perpendicular,
            -perpendicular,
            (direction + perpendicular * 1.4).normalize_or_zero(),
            (direction - perpendicular * 1.4).normalize_or_zero(),
        ];
        let gap = label_size * 0.55 + 7.0 * overlay_size / 180.0;
        centers[index] = candidates
            .into_iter()
            .enumerate()
            .map(|(candidate_index, candidate)| {
                let center = (endpoint
                    + candidate * (gap + candidate_index as f32 * overlay_size / 120.0))
                    .clamp(lower, upper);
                let axis_clearance = half_extent + 3.0 * overlay_size / 180.0;
                let hits_axis = projected.iter().any(|(_, axis_endpoint)| {
                    point_segment_distance_squared(center, origin, *axis_endpoint)
                        < axis_clearance * axis_clearance
                });
                let hits_origin = center.distance_squared(origin)
                    < (half_extent + 7.0 * overlay_size / 180.0).powi(2);
                let hits_label = centers[..index].iter().any(|other| {
                    let delta = (center - *other).abs();
                    delta.x < half_extent * 2.0 && delta.y < half_extent * 2.0
                });
                (
                    usize::from(hits_axis) + usize::from(hits_origin) + usize::from(hits_label),
                    candidate_index,
                    center,
                )
            })
            .min_by_key(|(collisions, candidate_index, _)| (*collisions, *candidate_index))
            .map(|(_, _, center)| center)
            .expect("axis label candidates are non-empty");
    }
    centers
}

fn point_segment_distance_squared(point: Vec2, start: Vec2, end: Vec2) -> f32 {
    let segment = end - start;
    let length_squared = segment.length_squared();
    if length_squared <= f32::EPSILON {
        return point.distance_squared(start);
    }
    let t = ((point - start).dot(segment) / length_squared).clamp(0.0, 1.0);
    point.distance_squared(start + segment * t)
}

pub(crate) fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut egui_textures: ResMut<EguiUserTextures>,
) {
    commands.spawn((
        Name::new("Exact-color background camera"),
        Camera2d,
        Camera {
            order: -2,
            clear_color: ClearColorConfig::Custom(Color::WHITE),
            ..default()
        },
        // This camera only clears the window to the exact requested background
        // colour. Without an empty layer mask it also receives layer-0 Gizmos,
        // producing a camera-fixed duplicate of the unit-cell lines.
        RenderLayers::none(),
        BackgroundCamera,
    ));

    commands.spawn((
        Name::new("Presentation camera"),
        Camera3d::default(),
        Camera {
            order: 0,
            clear_color: ClearColorConfig::Custom(Color::NONE),
            output_mode: CameraOutputMode::Write {
                // The transparent intermediate render target already contains
                // premultiplied colors. Using straight-alpha blending here
                // would multiply antialiased edge pixels by alpha a second
                // time and produce dark fringes.
                blend_state: Some(BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                clear_color: ClearColorConfig::None,
            },
            ..default()
        },
        Projection::Perspective(PerspectiveProjection::default()),
        Tonemapping::TonyMcMapface,
        Exposure { ev100: 7.5 },
        Msaa::Sample4,
        Transform::from_xyz(10.0, 8.0, 12.0).looking_at(Vec3::ZERO, Vec3::Y),
        OrbitCamera {
            target: Vec3::ZERO,
            radius: 18.0,
            rotation: Quat::from_euler(EulerRot::YXZ, -0.68, -0.42, 0.0),
            arcball_anchor: None,
        },
    ));

    let axis_colors =
        DEFAULT_AXIS_COLORS.map(|[red, green, blue]| Color::srgb_u8(red, green, blue));
    let axis_materials = axis_colors.map(|color| {
        materials.add(StandardMaterial {
            base_color: color,
            emissive: color.to_linear() * 0.08,
            metallic: 0.12,
            perceptual_roughness: 0.26,
            reflectance: 0.48,
            ..default()
        })
    });
    let shaft_mesh = meshes.add(Cylinder::new(0.065, 0.82).mesh().resolution(32));
    let head_mesh = meshes.add(Cone::new(0.16, 0.38).mesh().resolution(32));
    let origin_mesh = meshes.add(Sphere::new(0.18).mesh().ico(4).expect("valid icosphere"));
    let origin_material = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        emissive: LinearRgba::rgb(0.08, 0.08, 0.08),
        metallic: 0.08,
        perceptual_roughness: 0.32,
        ..default()
    });
    for (index, material) in axis_materials.iter().enumerate() {
        commands
            .spawn((
                Name::new(format!("Orientation axis {}", ["a", "b", "c"][index])),
                Transform::default(),
                Visibility::default(),
                OrientationAxisRoot(index),
            ))
            .with_children(|parent| {
                parent.spawn((
                    Mesh3d(shaft_mesh.clone()),
                    MeshMaterial3d(material.clone()),
                    Transform::from_xyz(0.0, 0.41, 0.0),
                    RenderLayers::layer(AXIS_RENDER_LAYER),
                ));
                parent.spawn((
                    Mesh3d(head_mesh.clone()),
                    MeshMaterial3d(material.clone()),
                    Transform::from_xyz(0.0, 1.01, 0.0),
                    RenderLayers::layer(AXIS_RENDER_LAYER),
                ));
            });
    }
    commands.spawn((
        Name::new("Orientation-axis origin"),
        Mesh3d(origin_mesh),
        MeshMaterial3d(origin_material),
        Transform::default(),
        RenderLayers::layer(AXIS_RENDER_LAYER),
    ));
    commands.insert_resource(OrientationAxisMaterials(axis_materials));
    let axis_image = images.add(Image::new_target_texture(
        AXIS_OVERLAY_SIZE,
        AXIS_OVERLAY_SIZE,
        TextureFormat::Rgba8UnormSrgb,
        None,
    ));
    let axis_texture_id = egui_textures.add_image(EguiTextureHandle::Strong(axis_image.clone()));
    commands.insert_resource(AxisOverlay {
        image: axis_image.clone(),
        texture_id: axis_texture_id,
    });
    commands.spawn((
        Name::new("3D orientation-axis camera"),
        Camera3d::default(),
        Camera {
            order: 1,
            clear_color: ClearColorConfig::Custom(Color::NONE),
            ..default()
        },
        RenderTarget::Image(axis_image.into()),
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::FixedVertical {
                viewport_height: 3.4,
            },
            ..OrthographicProjection::default_3d()
        }),
        Msaa::Sample4,
        Transform::from_xyz(0.0, 0.0, 4.0).looking_at(Vec3::ZERO, Vec3::Y),
        RenderLayers::layer(AXIS_RENDER_LAYER),
        OrientationAxisCamera,
    ));
    commands.spawn((
        Name::new("Orientation-axis key light"),
        DirectionalLight {
            color: Color::srgb(1.0, 0.97, 0.92),
            illuminance: 16_000.0,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.65, -0.75, 0.0)),
        RenderLayers::layer(AXIS_RENDER_LAYER),
        LightRole::AxisKey,
    ));
    commands.spawn((
        Name::new("Orientation-axis fill light"),
        DirectionalLight {
            color: Color::srgb(0.5, 0.68, 1.0),
            illuminance: 5_000.0,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, 0.45, 2.25, 0.0)),
        RenderLayers::layer(AXIS_RENDER_LAYER),
        LightRole::AxisFill,
    ));
    commands.spawn((
        Name::new("Interface camera"),
        Camera2d,
        Camera {
            order: 3,
            clear_color: ClearColorConfig::None,
            output_mode: CameraOutputMode::Write {
                // egui and paint callbacks are composited into a transparent
                // premultiplied intermediate target before this final write.
                blend_state: Some(BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                clear_color: ClearColorConfig::None,
            },
            ..default()
        },
        RenderLayers::none(),
        InterfaceCamera,
        PrimaryEguiContext,
    ));

    commands.spawn((
        Name::new("User light"),
        PointLight {
            color: Color::srgb(1.0, 0.94, 0.86),
            intensity: 760_000.0,
            range: 100.0,
            radius: 4.5,
            shadow_maps_enabled: true,
            shadow_depth_bias: 0.02,
            shadow_normal_bias: 0.55,
            ..default()
        },
        Transform::from_xyz(8.0, 10.0, 12.0),
        Visibility::Hidden,
        LightRole::KeyPoint,
    ));
    commands.spawn((
        Name::new("User spot light"),
        SpotLight {
            color: Color::srgb(1.0, 0.94, 0.86),
            intensity: 760_000.0,
            range: 100.0,
            radius: 4.5,
            inner_angle: 28.0_f32.to_radians(),
            outer_angle: 42.0_f32.to_radians(),
            shadow_maps_enabled: true,
            shadow_depth_bias: 0.02,
            shadow_normal_bias: 0.55,
            ..default()
        },
        Transform::from_xyz(8.0, 10.0, 12.0).looking_at(Vec3::ZERO, Vec3::Y),
        Visibility::Hidden,
        LightRole::KeySpot,
    ));
    commands.spawn((
        Name::new("User directional light"),
        DirectionalLight {
            color: Color::srgb(1.0, 0.94, 0.86),
            illuminance: 20_000.0,
            shadow_maps_enabled: true,
            shadow_depth_bias: 0.02,
            shadow_normal_bias: 0.55,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.6, 0.7, 0.0)),
        LightRole::KeyDirectional,
    ));
    commands.spawn((
        Name::new("Soft fill"),
        DirectionalLight {
            color: Color::srgb(0.52, 0.66, 1.0),
            illuminance: 4_800.0,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.8, 2.4, 0.0)),
        LightRole::Fill,
    ));
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn rebuild_legend_previews(
    mut commands: Commands,
    state: Res<AppState>,
    settings: Res<VisualSettings>,
    overrides: Res<ElementOverrides>,
    mut previews: ResMut<LegendPreviews>,
    old: Query<Entity, With<LegendPreviewVisual>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut egui_textures: ResMut<EguiUserTextures>,
) {
    let mut symbols: Vec<_> = state
        .structure
        .visible_atoms(settings.expand_symmetry)
        .iter()
        .filter(|atom| {
            overrides
                .0
                .get(&atom.element)
                .is_none_or(|style| style.visible)
        })
        .map(|atom| atom.element.clone())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    symbols.sort_unstable();
    let fingerprint = format!(
        "{:?}|{:.3}|{:.3}|{:.3}|{:?}|{:?}",
        symbols,
        settings.atom_metallic,
        settings.atom_roughness,
        settings.atom_reflectance,
        settings.element_color_scheme,
        overrides.0
    );
    if previews.fingerprint == fingerprint {
        return;
    }
    for entity in &old {
        commands.entity(entity).despawn();
    }
    previews.entries.clear();
    previews.fingerprint = fingerprint;
    if symbols.is_empty() {
        return;
    }

    let sphere = meshes.add(Sphere::new(0.82).mesh().ico(5).expect("valid icosphere"));
    commands.spawn((
        DirectionalLight {
            color: Color::srgb(1.0, 0.96, 0.90),
            illuminance: 16_000.0,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.7, -0.7, 0.0)),
        RenderLayers::layer(LEGEND_RENDER_LAYER),
        LegendPreviewVisual,
    ));
    for (index, symbol) in symbols.into_iter().enumerate() {
        let x = index as f32 * 5.0;
        let style = resolved_style(&symbol, &overrides, settings.element_color_scheme);
        let material = materials.add(StandardMaterial {
            base_color: style.color,
            emissive: style.color.to_linear() * 0.035,
            metallic: settings.atom_metallic,
            perceptual_roughness: settings.atom_roughness,
            reflectance: settings.atom_reflectance,
            ..default()
        });
        commands.spawn((
            Mesh3d(sphere.clone()),
            MeshMaterial3d(material),
            Transform::from_xyz(x, 0.0, 0.0),
            RenderLayers::layer(LEGEND_RENDER_LAYER),
            LegendPreviewVisual,
        ));
        let image = images.add(Image::new_target_texture(
            LEGEND_ICON_SIZE,
            LEGEND_ICON_SIZE,
            TextureFormat::Rgba8UnormSrgb,
            None,
        ));
        let texture_id = egui_textures.add_image(EguiTextureHandle::Strong(image.clone()));
        commands.spawn((
            Camera3d::default(),
            Camera {
                order: -1,
                clear_color: ClearColorConfig::Custom(Color::srgba(0.0, 0.0, 0.0, 0.0)),
                ..default()
            },
            RenderTarget::Image(image.clone().into()),
            Projection::Orthographic(OrthographicProjection {
                scaling_mode: ScalingMode::FixedVertical {
                    viewport_height: 2.35,
                },
                ..OrthographicProjection::default_3d()
            }),
            Transform::from_xyz(x, 0.0, 4.0).looking_at(Vec3::new(x, 0.0, 0.0), Vec3::Y),
            RenderLayers::layer(LEGEND_RENDER_LAYER),
            LegendPreviewVisual,
        ));
        previews.entries.insert(
            symbol,
            LegendPreview {
                texture_id,
                _image: image,
            },
        );
    }
}

#[allow(clippy::type_complexity)]
pub(crate) fn sync_legend_preview_lights(
    main_camera: Single<
        &Transform,
        (
            With<OrbitCamera>,
            Without<LegendPreviewVisual>,
            Without<LightRole>,
        ),
    >,
    main_lights: Query<(&LightRole, &Transform), Without<LegendPreviewVisual>>,
    preview_cameras: Query<
        &Transform,
        (With<Camera>, With<LegendPreviewVisual>, Without<LightRole>),
    >,
    mut preview_lights: Query<
        &mut Transform,
        (
            With<LegendPreviewVisual>,
            With<DirectionalLight>,
            Without<Camera>,
            Without<LightRole>,
        ),
    >,
) {
    let Some(preview_camera) = preview_cameras.iter().next() else {
        return;
    };
    let key_rotation = main_lights.iter().find_map(|(role, transform)| {
        matches!(role, LightRole::KeyDirectional).then_some(transform.rotation)
    });
    for mut transform in &mut preview_lights {
        let Some(main_light_rotation) = key_rotation else {
            continue;
        };

        // Translate the light from the main camera's coordinate system into the
        // fixed preview camera's coordinate system.  Camera-following lights
        // therefore keep the same screen-space direction; world-fixed lights
        // naturally move across the preview sphere when the main camera rotates.
        transform.rotation =
            preview_camera.rotation * main_camera.rotation.inverse() * main_light_rotation;
    }
}

#[allow(clippy::type_complexity)]
pub(crate) fn orient_occupancy_sectors(
    camera: Single<(&Transform, &Projection), (With<OrbitCamera>, Without<OccupancySector>)>,
    mut sectors: Query<&mut Transform, With<OccupancySector>>,
) {
    let (camera_transform, projection) = camera.into_inner();
    let camera_up = camera_transform.rotation * Vec3::Y;
    // An orthographic camera has no meaningful position: every surface should
    // face the shared view direction.  In particular, do not use the orbit
    // distance here, because it intentionally snaps back when the projection
    // slider reaches its parallel endpoint.
    let orthographic_view_direction = matches!(projection, Projection::Orthographic(_))
        .then(|| camera_transform.rotation * Vec3::Z);
    for mut transform in &mut sectors {
        let rotation = if let Some(toward_camera) = orthographic_view_direction {
            occupancy_sector_rotation_for_view(toward_camera, camera_up)
        } else {
            occupancy_sector_rotation(
                camera_transform.translation,
                camera_up,
                transform.translation,
            )
        };
        if let Some(rotation) = rotation {
            transform.rotation = rotation;
        }
    }
}

/// Keeps the sector's local +Z (the centre of its visible fan) facing the
/// camera. This is computed per site instead of inheriting the camera's roll,
/// so perspective views also remain correct away from the orbit target.
fn occupancy_sector_rotation(
    camera_position: Vec3,
    camera_up: Vec3,
    sector_position: Vec3,
) -> Option<Quat> {
    let toward_camera = (camera_position - sector_position).normalize_or_zero();
    occupancy_sector_rotation_for_view(toward_camera, camera_up)
}

fn occupancy_sector_rotation_for_view(toward_camera: Vec3, camera_up: Vec3) -> Option<Quat> {
    (toward_camera.length_squared() > 1.0e-8).then(|| {
        Transform::IDENTITY
            .looking_to(-toward_camera, camera_up)
            .rotation
    })
}

/// Synchronize the dedicated screen-space line configuration from the UI.
///
/// This must be separate from `Gizmos<UnitCellGizmoGroup>` access: Gizmos
/// reads the same config store internally.
pub(crate) fn sync_unit_cell_gizmo_config(
    mut config_store: ResMut<GizmoConfigStore>,
    settings: Res<VisualSettings>,
) {
    let (config, _) = config_store.config_mut::<UnitCellGizmoGroup>();
    config.line.width = settings.unit_cell_line_width.clamp(0.25, 8.0);
    config.line.perspective = false;
    config.line.style = match settings.unit_cell_line_style {
        CellLineStyle::Solid => GizmoLineStyle::Solid,
        CellLineStyle::Dotted => GizmoLineStyle::Dotted,
        CellLineStyle::Dashed => GizmoLineStyle::Dashed {
            gap_scale: 0.60,
            line_scale: 1.00,
        },
    };
    config.depth_bias = 0.0;
}

/// Draw unit-cell edges through Bevy's screen-space gizmo renderer.
///
/// The renderer expands line segments after projection, making `width` a
/// physical-pixel value that remains stable during camera zoom and projection
/// changes. Gizmos retain depth testing, so opaque atoms still occlude them.
pub(crate) fn draw_unit_cell_lines(
    mut gizmos: Gizmos<UnitCellGizmoGroup>,
    state: Res<AppState>,
    settings: Res<VisualSettings>,
) {
    if settings.unit_cell_display == UnitCellDisplay::Hidden {
        return;
    }

    let supercell = settings.supercell.map(|value| value.max(1));
    let primitive_vectors = state.structure.cell.vectors.map(crystal_to_world);
    let total_vector = primitive_vectors[0] * supercell[0] as f32
        + primitive_vectors[1] * supercell[1] as f32
        + primitive_vectors[2] * supercell[2] as f32;
    let center = total_vector * 0.5;
    let color = Color::srgb_u8(
        settings.unit_cell_color[0],
        settings.unit_cell_color[1],
        settings.unit_cell_color[2],
    );

    let mut draw_edge = |from: Vec3, to: Vec3| gizmos.line(from - center, to - center, color);
    match settings.unit_cell_display {
        UnitCellDisplay::Hidden => {}
        UnitCellDisplay::Single => {
            for (from, to) in cell_edges(primitive_vectors) {
                draw_edge(from, to);
            }
        }
        UnitCellDisplay::All => {
            let mut drawn_edges = HashSet::new();
            for ix in 0..supercell[0] {
                for iy in 0..supercell[1] {
                    for iz in 0..supercell[2] {
                        let offset = primitive_vectors[0] * ix as f32
                            + primitive_vectors[1] * iy as f32
                            + primitive_vectors[2] * iz as f32;
                        for (from, to) in cell_edges(primitive_vectors) {
                            let from = from + offset;
                            let to = to + offset;
                            if drawn_edges.insert(cell_edge_key(from, to)) {
                                draw_edge(from, to);
                            }
                        }
                    }
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(crate) fn rebuild_scene(
    mut commands: Commands,
    mut dirty: ResMut<SceneDirty>,
    state: Res<AppState>,
    settings: Res<VisualSettings>,
    overrides: Res<ElementOverrides>,
    mut selection: ResMut<SelectionState>,
    mut scene_status: ResMut<SceneStatus>,
    old_visuals: Query<Entity, With<CrystalVisual>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut gizmo_assets: ResMut<Assets<GizmoAsset>>,
    mut camera: Single<(&mut OrbitCamera, &mut Transform), With<OrbitCamera>>,
) {
    if !dirty.rebuild {
        return;
    }
    dirty.rebuild = false;
    let source_atoms: Vec<_> = state
        .structure
        .visible_atoms(settings.expand_symmetry)
        .iter()
        .filter(|atom| {
            overrides
                .0
                .get(&atom.element)
                .is_none_or(|appearance| appearance.visible)
        })
        .collect();
    let supercell = settings.supercell.map(|value| value.max(1));
    let requested_atoms = source_atoms
        .iter()
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
    if requested_atoms > MAX_RENDER_ATOMS {
        scene_status.warning = Some(format!(
            "Requested supercell contains {requested_atoms} atoms. The previous scene is kept; reduce A × B × C below {MAX_RENDER_ATOMS} displayed atoms."
        ));
        dirty.reset_camera = false;
        return;
    }
    scene_status.warning = None;
    for entity in &old_visuals {
        commands.entity(entity).despawn();
    }

    let sphere_mesh = meshes.add(Sphere::new(1.0).mesh().ico(5).expect("valid icosphere"));
    let cylinder_mesh = meshes.add(Cylinder::new(1.0, 1.0).mesh().resolution(24));
    let arrow_head_mesh = meshes.add(Cone::new(0.16, 0.42).mesh().resolution(24));
    let mut atom_materials: HashMap<String, Handle<StandardMaterial>> = HashMap::new();
    let background_is_light = settings.background.iter().map(|v| *v as u32).sum::<u32>() > 384;
    let outline_material = materials.add(StandardMaterial {
        base_color: if background_is_light {
            Color::srgb(0.055, 0.065, 0.09)
        } else {
            Color::srgb(0.004, 0.006, 0.012)
        },
        unlit: true,
        cull_mode: Some(Face::Front),
        ..default()
    });
    let structure = &state.structure;
    let total_vector = structure.cell.vectors[0] * supercell[0] as f32
        + structure.cell.vectors[1] * supercell[1] as f32
        + structure.cell.vectors[2] * supercell[2] as f32;
    let center = crystal_to_world(total_vector * 0.5);
    let mut render_atoms = Vec::with_capacity(requested_atoms as usize);
    for atom in &source_atoms {
        let fractional = canonical_fractional(atom.fractional);
        let offsets = if settings.show_boundary_atoms {
            periodic_image_offsets(fractional, supercell)
        } else {
            interior_cell_offsets(supercell)
        };
        for offset in offsets {
            render_atoms.push(make_render_atom(
                atom,
                fractional + offset,
                &structure.cell,
                center,
            ));
        }
    }
    let inside_atom_count = render_atoms.len();

    selection
        .atoms
        .retain(|selected| render_atoms.iter().any(|atom| selected.matches(atom)));
    let selection_material = materials.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.74, 0.10),
        emissive: LinearRgba::rgb(1.0, 0.38, 0.015),
        unlit: true,
        cull_mode: Some(Face::Front),
        ..default()
    });
    let wireframe_fill_material = materials.add(StandardMaterial {
        base_color: Color::srgba(1.0, 1.0, 1.0, 0.0),
        alpha_mode: AlphaMode::Blend,
        ..default()
    });
    if settings.bond_rules.iter().any(|rule| {
        rule.enabled
            && matches!(
                rule.boundary_mode,
                BondBoundaryMode::SearchAdditionalForA1
                    | BondBoundaryMode::SearchRecursivelyIfEitherVisible
            )
    }) {
        for atom in &source_atoms {
            let fractional = canonical_fractional(atom.fractional);
            for ix in -1..=supercell[0] as i32 {
                for iy in -1..=supercell[1] as i32 {
                    for iz in -1..=supercell[2] as i32 {
                        let candidate = fractional + Vec3::new(ix as f32, iy as f32, iz as f32);
                        let outside = candidate.x < 0.0
                            || candidate.y < 0.0
                            || candidate.z < 0.0
                            || candidate.x > supercell[0] as f32
                            || candidate.y > supercell[1] as f32
                            || candidate.z > supercell[2] as f32;
                        if outside {
                            render_atoms.push(make_render_atom(
                                atom,
                                candidate,
                                &structure.cell,
                                center,
                            ));
                        }
                    }
                }
            }
        }
    }

    let bonds = if settings.show_bonds {
        find_bonds(&render_atoms, &settings)
    } else {
        Vec::new()
    };
    let external_bond_atoms: HashSet<_> = bonds
        .iter()
        .filter(|&&(first, second)| {
            bond_is_displayed(&render_atoms[first], &render_atoms[second], &settings)
                || bond_supports_polyhedron(&render_atoms[first], &render_atoms[second], &settings)
        })
        .flat_map(|&(first, second)| [first, second])
        .filter(|&index| index >= inside_atom_count)
        .collect();
    let mut visible_indices: Vec<_> = (0..inside_atom_count).collect();
    visible_indices.extend(external_bond_atoms.iter().copied());
    visible_indices.sort_unstable();
    visible_indices.dedup();
    if settings.show_elements {
        for group in render_atom_site_groups(&render_atoms, &visible_indices) {
            spawn_render_atom_site(
                &mut commands,
                &mut meshes,
                &mut materials,
                &sphere_mesh,
                &selection_material,
                &outline_material,
                &wireframe_fill_material,
                &mut atom_materials,
                &group,
                &overrides,
                &settings,
                &selection,
            );
        }
    }
    if settings.show_elements && settings.show_thermal_ellipsoids {
        let mut ellipsoid_materials = HashMap::new();
        for &index in &visible_indices {
            let atom = &render_atoms[index];
            let Some(displacement) = atom.anisotropic_u else {
                continue;
            };
            let style = resolved_style(&atom.element, &overrides, settings.element_color_scheme);
            let material = ellipsoid_materials
                .entry(atom.element.clone())
                .or_insert_with(|| thermal_ellipsoid_material(&mut materials, style, &settings))
                .clone();
            if let Some(transform) = thermal_ellipsoid_transform(
                &structure.cell,
                displacement,
                atom.position,
                settings.thermal_ellipsoid_scale,
            ) {
                commands.spawn((
                    Name::new(format!("{} {} thermal ellipsoid", atom.element, atom.label)),
                    Mesh3d(sphere_mesh.clone()),
                    MeshMaterial3d(material),
                    transform,
                    CrystalVisual,
                ));
            }
        }
    }
    if settings.show_bonds && settings.atom_display_mode != AtomDisplayMode::SpaceFilling {
        // Bicolor bonds reuse the element material. Populate this cache even
        // when the atoms themselves are globally hidden.
        for atom in &render_atoms {
            let style = resolved_style(&atom.element, &overrides, settings.element_color_scheme);
            atom_materials
                .entry(atom.element.clone())
                .or_insert_with(|| create_atom_material(&mut materials, style, &settings));
        }
        let mut bond_line_assets: HashMap<usize, GizmoAsset> = HashMap::new();
        for &(a, b) in &bonds {
            let first = &render_atoms[a];
            let second = &render_atoms[b];
            let Some((rule_index, rule)) = displayed_bond_rule(first, second, &settings) else {
                continue;
            };
            match rule.bond_style {
                BondStyle::UnicolorCylinder | BondStyle::BicolorCylinder => {
                    spawn_cylinder_bond(
                        &mut commands,
                        &mut materials,
                        &cylinder_mesh,
                        atom_materials[&first.element].clone(),
                        atom_materials[&second.element].clone(),
                        first.position,
                        second.position,
                        rule,
                    );
                }
                BondStyle::ColorLine | BondStyle::DottedLine | BondStyle::DashedLine => {
                    bond_line_assets.entry(rule_index).or_default().line(
                        first.position,
                        second.position,
                        Color::srgb_u8(rule.bond_color[0], rule.bond_color[1], rule.bond_color[2]),
                    );
                }
                BondStyle::GradientLine => {
                    let first_color =
                        resolved_style(&first.element, &overrides, settings.element_color_scheme)
                            .color;
                    let second_color =
                        resolved_style(&second.element, &overrides, settings.element_color_scheme)
                            .color;
                    bond_line_assets
                        .entry(rule_index)
                        .or_default()
                        .line_gradient(first.position, second.position, first_color, second_color);
                }
            }
        }
        for (rule_index, asset) in bond_line_assets {
            let rule = &settings.bond_rules[rule_index];
            let style = bond_gizmo_line_style(rule.bond_style)
                .expect("only line bond styles create a retained gizmo");
            commands.spawn((
                Name::new(format!("Bond line rule {}", rule_index + 1)),
                Gizmo {
                    handle: gizmo_assets.add(asset),
                    line_config: GizmoLineConfig {
                        width: rule.bond_pixel_width.clamp(0.5, 8.0),
                        perspective: false,
                        style,
                        ..default()
                    },
                    depth_bias: 0.0,
                },
                CrystalVisual,
            ));
        }
    }

    if settings.show_bonds
        && settings
            .bond_rules
            .iter()
            .any(|rule| rule.enabled && rule.show_polyhedra)
    {
        let mut adjacency = vec![Vec::new(); render_atoms.len()];
        for &(first, second) in &bonds {
            adjacency[first].push(second);
            adjacency[second].push(first);
        }
        let mut polyhedron_edges = settings.polyhedron_show_edges.then(GizmoAsset::default);
        let mut has_polyhedron_edges = false;
        for (rule_index, rule) in settings.bond_rules.iter().enumerate() {
            if !rule.enabled || !rule.show_polyhedra {
                continue;
            }
            for (center_index, center_atom) in render_atoms.iter().enumerate() {
                if !atom_matches(&rule.center, center_atom, rule.match_mode) {
                    continue;
                }
                let neighbors: Vec<_> = adjacency[center_index]
                    .iter()
                    .copied()
                    .filter(|&neighbor_index| {
                        rule_matches_directed(center_atom, &render_atoms[neighbor_index], rule)
                    })
                    .map(|neighbor_index| render_atoms[neighbor_index].position)
                    .collect();
                if let Some(polyhedron) = coordination_polyhedron(&neighbors) {
                    let color = resolved_polyhedron_color(
                        rule,
                        center_atom,
                        &overrides,
                        settings.element_color_scheme,
                    );
                    let polyhedron_material =
                        materials.add(polyhedron_material(color, rule.polyhedron_opacity));
                    commands.spawn((
                        Name::new(format!(
                            "Coordination polyhedron {} for {}",
                            rule_index + 1,
                            center_atom.label
                        )),
                        Mesh3d(meshes.add(polyhedron.mesh)),
                        MeshMaterial3d(polyhedron_material.clone()),
                        CrystalVisual,
                    ));
                    if let Some(edges) = &mut polyhedron_edges {
                        for (start, end) in polyhedron.edges {
                            edges.line(
                                start,
                                end,
                                Color::srgb_u8(
                                    settings.polyhedron_edge_color[0],
                                    settings.polyhedron_edge_color[1],
                                    settings.polyhedron_edge_color[2],
                                ),
                            );
                            has_polyhedron_edges = true;
                        }
                    }
                }
            }
        }
        if has_polyhedron_edges && let Some(asset) = polyhedron_edges {
            commands.spawn((
                Name::new("Polyhedron edges"),
                Gizmo {
                    handle: gizmo_assets.add(asset),
                    line_config: GizmoLineConfig {
                        width: settings.polyhedron_edge_radius.clamp(0.5, 8.0),
                        perspective: false,
                        style: GizmoLineStyle::Solid,
                        ..default()
                    },
                    depth_bias: 0.0,
                },
                CrystalVisual,
            ));
        }
    }

    if settings.show_vectors {
        for vector in settings
            .vectors
            .layers
            .iter()
            .filter(|vector| vector.enabled)
        {
            let material = materials.add(StandardMaterial {
                base_color: Color::srgb_u8(
                    vector.style.color[0],
                    vector.style.color[1],
                    vector.style.color[2],
                ),
                emissive: Color::srgb_u8(
                    vector.style.color[0],
                    vector.style.color[1],
                    vector.style.color[2],
                )
                .to_linear()
                    * 0.10,
                metallic: 0.12,
                perceptual_roughness: 0.24,
                ..default()
            });
            for (_, atom) in render_atoms.iter().enumerate().filter(|(index, atom)| {
                (*index < inside_atom_count || external_bond_atoms.contains(index))
                    && vector_matches_atom(&vector.target, atom)
            }) {
                let direction = resolved_vector_direction(vector, atom, &structure.cell);
                let radius =
                    resolved_style(&atom.element, &overrides, settings.element_color_scheme)
                        .display_radius
                        * settings.atom_scale
                        * atom.occupancy.max(0.15).cbrt();
                spawn_atom_vector(
                    &mut commands,
                    cylinder_mesh.clone(),
                    arrow_head_mesh.clone(),
                    material.clone(),
                    atom.position,
                    direction,
                    radius,
                    &vector.style,
                    settings.vectors.display_scale,
                );
            }
        }
    }

    let world_vectors = [
        crystal_to_world(structure.cell.vectors[0] * supercell[0] as f32),
        crystal_to_world(structure.cell.vectors[1] * supercell[1] as f32),
        crystal_to_world(structure.cell.vectors[2] * supercell[2] as f32),
    ];
    let extent = world_vectors
        .iter()
        .map(|value| value.length())
        .fold(1.0_f32, f32::max);
    if settings.show_scene_objects {
        spawn_scene_objects(
            &mut commands,
            &mut meshes,
            &mut materials,
            &mut gizmo_assets,
            &settings.scene_objects.objects,
            structure,
            center,
            sphere_mesh.clone(),
            cylinder_mesh.clone(),
            arrow_head_mesh,
        );
    }
    let reset = dirty.reset_camera;
    dirty.reset_camera = false;
    if reset {
        let (orbit, transform) = &mut *camera;
        orbit.radius = (extent * 2.05 + 4.0).clamp(7.0, 500.0);
        orbit.reset();
        update_camera_transform(orbit, transform);
    }
}

#[allow(clippy::too_many_arguments)]
fn spawn_scene_objects(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    gizmo_assets: &mut Assets<GizmoAsset>,
    objects: &[SceneObject],
    structure: &CrystalStructure,
    crystal_center: Vec3,
    sphere_mesh: Handle<Mesh>,
    cylinder_mesh: Handle<Mesh>,
    arrow_head_mesh: Handle<Mesh>,
) {
    for object in objects.iter().filter(|object| object.visible) {
        let position = scene_object_position(object, structure, crystal_center);
        let rotation = Quat::from_euler(
            EulerRot::XYZ,
            object.rotation[0].to_radians(),
            object.rotation[1].to_radians(),
            object.rotation[2].to_radians(),
        );
        let color = Color::srgb_u8(object.color[0], object.color[1], object.color[2]);
        let mut material = |metallic: f32, roughness: f32| {
            materials.add(StandardMaterial {
                base_color: color,
                metallic,
                perceptual_roughness: roughness,
                alpha_mode: AlphaMode::Opaque,
                double_sided: true,
                cull_mode: None,
                ..default()
            })
        };

        match &object.kind {
            SceneObjectKind::Ground {
                size,
                metallic,
                roughness,
            } => {
                commands.spawn((
                    Name::new(format!("Scene object · {}", object.name)),
                    Mesh3d(meshes.add(Plane3d::default().mesh().size(*size, *size))),
                    MeshMaterial3d(material(*metallic, *roughness)),
                    Transform::from_translation(position).with_rotation(rotation),
                    CrystalVisual,
                ));
            }
            SceneObjectKind::Label { .. } => {}
            SceneObjectKind::CrystalPlane {
                miller,
                offset,
                size,
                show_normal,
                normal_length,
            } => {
                let Some((normal, offset_vector)) =
                    miller_plane_normal(&structure.cell, *miller, *offset)
                else {
                    continue;
                };
                let origin = position + rotation * offset_vector;
                let plane_rotation = rotation * Quat::from_rotation_arc(Vec3::Y, normal);
                let plane_material = material(0.0, 0.55);
                commands.spawn((
                    Name::new(format!("Scene object · {}", object.name)),
                    Mesh3d(meshes.add(Plane3d::default().mesh().size(*size, *size))),
                    MeshMaterial3d(plane_material.clone()),
                    Transform::from_translation(origin).with_rotation(plane_rotation),
                    CrystalVisual,
                ));
                if *show_normal {
                    spawn_scene_arrow(
                        commands,
                        cylinder_mesh.clone(),
                        arrow_head_mesh.clone(),
                        plane_material,
                        origin,
                        rotation * normal * *normal_length,
                        0.045,
                        0.25,
                        0.12,
                    );
                }
            }
            SceneObjectKind::Arrow {
                direction,
                shaft_radius,
                head_length,
                head_radius,
            } => {
                let vector = match direction {
                    ArrowDirection::Cartesian(values) => Vec3::from_array(*values),
                    ArrowDirection::CrystalDirection(indices) => {
                        crystal_to_world(structure.cell.fractional_to_cartesian(Vec3::new(
                            indices[0] as f32,
                            indices[1] as f32,
                            indices[2] as f32,
                        )))
                    }
                    ArrowDirection::PlaneNormal(indices) => {
                        let Some((normal, _)) = miller_plane_normal(&structure.cell, *indices, 0.0)
                        else {
                            continue;
                        };
                        normal
                    }
                };
                if vector.length_squared() < 1.0e-8 {
                    continue;
                }
                spawn_scene_arrow(
                    commands,
                    cylinder_mesh.clone(),
                    arrow_head_mesh.clone(),
                    material(0.12, 0.3),
                    position,
                    rotation * vector,
                    *shaft_radius,
                    *head_length,
                    *head_radius,
                );
            }
            SceneObjectKind::Marker { radius } | SceneObjectKind::Sphere { radius } => {
                commands.spawn((
                    Name::new(format!("Scene object · {}", object.name)),
                    Mesh3d(sphere_mesh.clone()),
                    MeshMaterial3d(material(0.08, 0.34)),
                    Transform::from_translation(position).with_scale(Vec3::splat(*radius)),
                    CrystalVisual,
                ));
            }
            SceneObjectKind::Box { size } => {
                commands.spawn((
                    Name::new(format!("Scene object · {}", object.name)),
                    Mesh3d(meshes.add(Cuboid::new(size[0], size[1], size[2]))),
                    MeshMaterial3d(material(0.08, 0.38)),
                    Transform::from_translation(position).with_rotation(rotation),
                    CrystalVisual,
                ));
            }
            SceneObjectKind::Cylinder { radius, height } => {
                commands.spawn((
                    Name::new(format!("Scene object · {}", object.name)),
                    Mesh3d(cylinder_mesh.clone()),
                    MeshMaterial3d(material(0.08, 0.34)),
                    Transform::from_translation(position)
                        .with_rotation(rotation)
                        .with_scale(Vec3::new(*radius, *height, *radius)),
                    CrystalVisual,
                ));
            }
            SceneObjectKind::Cone { radius, height } => {
                commands.spawn((
                    Name::new(format!("Scene object · {}", object.name)),
                    Mesh3d(meshes.add(Cone::new(*radius, *height).mesh().resolution(32))),
                    MeshMaterial3d(material(0.08, 0.34)),
                    Transform::from_translation(position).with_rotation(rotation),
                    CrystalVisual,
                ));
            }
            SceneObjectKind::BoundingBox { size, line_radius } => {
                let box_material = material(0.0, 0.4);
                for (start, end) in object_box_edges(*size) {
                    spawn_cylinder_between(
                        commands,
                        cylinder_mesh.clone(),
                        box_material.clone(),
                        position + rotation * start,
                        position + rotation * end,
                        *line_radius,
                    );
                }
            }
            SceneObjectKind::Line {
                start,
                end,
                radius,
                render_mode,
                screen_width,
            } => {
                let start =
                    position + rotation * scene_object_vector(*start, object.anchor, structure);
                let end = position + rotation * scene_object_vector(*end, object.anchor, structure);
                match render_mode {
                    SceneLineRenderMode::ThreeDimensional => spawn_cylinder_between(
                        commands,
                        cylinder_mesh.clone(),
                        material(0.0, 0.35),
                        start,
                        end,
                        *radius,
                    ),
                    SceneLineRenderMode::ScreenSpace => {
                        let mut asset = GizmoAsset::default();
                        asset.line(start, end, color);
                        commands.spawn((
                            Name::new(format!("Scene object · {}", object.name)),
                            Gizmo {
                                handle: gizmo_assets.add(asset),
                                line_config: GizmoLineConfig {
                                    width: screen_width.clamp(0.5, 32.0),
                                    perspective: false,
                                    style: GizmoLineStyle::Solid,
                                    ..default()
                                },
                                depth_bias: 0.0,
                            },
                            CrystalVisual,
                        ));
                    }
                }
            }
            SceneObjectKind::Polyline {
                points,
                radius,
                render_mode,
                screen_width,
            } => match render_mode {
                SceneLineRenderMode::ThreeDimensional => {
                    let line_material = material(0.0, 0.35);
                    for pair in points.windows(2) {
                        let start = position
                            + rotation * scene_object_vector(pair[0], object.anchor, structure);
                        let end = position
                            + rotation * scene_object_vector(pair[1], object.anchor, structure);
                        spawn_cylinder_between(
                            commands,
                            cylinder_mesh.clone(),
                            line_material.clone(),
                            start,
                            end,
                            *radius,
                        );
                    }
                }
                SceneLineRenderMode::ScreenSpace => {
                    let mut asset = GizmoAsset::default();
                    for pair in points.windows(2) {
                        let start = position
                            + rotation * scene_object_vector(pair[0], object.anchor, structure);
                        let end = position
                            + rotation * scene_object_vector(pair[1], object.anchor, structure);
                        asset.line(start, end, color);
                    }
                    if points.len() >= 2 {
                        commands.spawn((
                            Name::new(format!("Scene object · {}", object.name)),
                            Gizmo {
                                handle: gizmo_assets.add(asset),
                                line_config: GizmoLineConfig {
                                    width: screen_width.clamp(0.5, 32.0),
                                    perspective: false,
                                    style: GizmoLineStyle::Solid,
                                    ..default()
                                },
                                depth_bias: 0.0,
                            },
                            CrystalVisual,
                        ));
                    }
                }
            },
            SceneObjectKind::Measurement {
                kind,
                points,
                radius,
                ..
            } => {
                spawn_measurement(
                    commands,
                    cylinder_mesh.clone(),
                    material(0.0, 0.3),
                    *kind,
                    points,
                    *radius,
                    &structure.cell,
                    crystal_center,
                );
            }
        }
    }
}

pub(crate) fn scene_object_position(
    object: &SceneObject,
    structure: &CrystalStructure,
    crystal_center: Vec3,
) -> Vec3 {
    let local = Vec3::from_array(object.position);
    match object.anchor {
        SceneObjectAnchor::World => local,
        SceneObjectAnchor::Crystal => {
            crystal_to_world(structure.cell.fractional_to_cartesian(local)) - crystal_center
        }
        SceneObjectAnchor::Atom { site_id } => structure
            .atoms
            .iter()
            .find(|site| site.site_id == site_id)
            .map_or(local, |site| {
                crystal_to_world(
                    structure
                        .cell
                        .fractional_to_cartesian(site.fractional + local),
                ) - crystal_center
            }),
    }
}

#[allow(clippy::too_many_arguments)]
fn spawn_scene_arrow(
    commands: &mut Commands,
    cylinder_mesh: Handle<Mesh>,
    head_mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
    origin: Vec3,
    vector: Vec3,
    shaft_radius: f32,
    head_length: f32,
    head_radius: f32,
) {
    let length = vector.length();
    if length < 1.0e-5 {
        return;
    }
    let direction = vector / length;
    let head_length = head_length.min(length * 0.8);
    let shaft_end = origin + direction * (length - head_length);
    spawn_cylinder_between(
        commands,
        cylinder_mesh,
        material.clone(),
        origin,
        shaft_end,
        shaft_radius,
    );
    commands.spawn((
        Mesh3d(head_mesh),
        MeshMaterial3d(material),
        Transform::from_translation(shaft_end + direction * (head_length * 0.5))
            .with_rotation(Quat::from_rotation_arc(Vec3::Y, direction))
            .with_scale(Vec3::new(
                head_radius / 0.16,
                head_length / 0.42,
                head_radius / 0.16,
            )),
        CrystalVisual,
    ));
}

/// Returns the unit normal in renderer coordinates and the renderer-space
/// translation to the plane `h x + k y + l z = offset`.
fn miller_plane_normal(cell: &UnitCell, indices: [i32; 3], offset: f32) -> Option<(Vec3, Vec3)> {
    let [a, b, c] = cell.vectors;
    let volume = a.dot(b.cross(c));
    if volume.abs() < 1.0e-8 {
        return None;
    }
    // The reciprocal basis deliberately omits 2π: Miller planes obey
    // h·f = n in fractional coordinates, not a diffraction wavevector scale.
    let reciprocal = [
        b.cross(c) / volume,
        c.cross(a) / volume,
        a.cross(b) / volume,
    ];
    let normal_cartesian = reciprocal[0] * indices[0] as f32
        + reciprocal[1] * indices[1] as f32
        + reciprocal[2] * indices[2] as f32;
    let normal_squared = normal_cartesian.length_squared();
    if normal_squared < 1.0e-8 {
        return None;
    }
    Some((
        crystal_to_world(normal_cartesian.normalize()),
        crystal_to_world(normal_cartesian * (offset / normal_squared)),
    ))
}

#[allow(clippy::too_many_arguments)]
fn spawn_measurement(
    commands: &mut Commands,
    cylinder_mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
    kind: MeasurementKind,
    points: &[MeasurementPoint],
    radius: f32,
    cell: &UnitCell,
    crystal_center: Vec3,
) {
    if points.len() != kind.point_count() {
        return;
    }
    let points: Vec<_> = points
        .iter()
        .map(|point| {
            crystal_to_world(cell.fractional_to_cartesian(Vec3::from_array(point.fractional)))
                - crystal_center
        })
        .collect();
    let mut add_segment = |start: Vec3, end: Vec3| {
        spawn_cylinder_between(
            commands,
            cylinder_mesh.clone(),
            material.clone(),
            start,
            end,
            radius,
        );
    };
    match kind {
        MeasurementKind::Distance => {
            add_segment(points[0], points[1]);
        }
        MeasurementKind::Angle => {
            add_segment(points[1], points[0]);
            add_segment(points[1], points[2]);
        }
        MeasurementKind::Dihedral => {
            add_segment(points[0], points[1]);
            add_segment(points[1], points[2]);
            add_segment(points[2], points[3]);
        }
    }
}

pub(crate) fn measurement_label(
    kind: MeasurementKind,
    points: &[MeasurementPoint],
    cell: &UnitCell,
    crystal_center: Vec3,
) -> Option<(String, Vec3)> {
    if points.len() != kind.point_count() {
        return None;
    }
    let points: Vec<_> = points
        .iter()
        .map(|point| {
            crystal_to_world(cell.fractional_to_cartesian(Vec3::from_array(point.fractional)))
                - crystal_center
        })
        .collect();
    match kind {
        MeasurementKind::Distance => Some((
            format!("{:.3} Å", points[0].distance(points[1])),
            (points[0] + points[1]) * 0.5,
        )),
        MeasurementKind::Angle => {
            let left = (points[0] - points[1]).normalize_or_zero();
            let right = (points[2] - points[1]).normalize_or_zero();
            (left.length_squared() > 1.0e-8 && right.length_squared() > 1.0e-8).then(|| {
                let angle = left.dot(right).clamp(-1.0, 1.0).acos().to_degrees();
                let direction = (left + right).normalize_or_zero();
                (
                    format!("{angle:.2}°"),
                    points[1]
                        + direction
                            * points[1]
                                .distance(points[0])
                                .min(points[1].distance(points[2]))
                            * 0.42,
                )
            })
        }
        MeasurementKind::Dihedral => {
            let normal_a = (points[1] - points[0])
                .cross(points[2] - points[1])
                .normalize_or_zero();
            let normal_b = (points[2] - points[1])
                .cross(points[3] - points[2])
                .normalize_or_zero();
            let axis = (points[2] - points[1]).normalize_or_zero();
            (normal_a.length_squared() > 1.0e-8
                && normal_b.length_squared() > 1.0e-8
                && axis.length_squared() > 1.0e-8)
                .then(|| {
                    let angle = axis
                        .dot(normal_a.cross(normal_b))
                        .atan2(normal_a.dot(normal_b))
                        .to_degrees();
                    (
                        format!("{angle:.2}°"),
                        (points[1] + points[2]) * 0.5
                            + (normal_a + normal_b).normalize_or_zero() * 0.35,
                    )
                })
        }
    }
}

fn scene_object_vector(
    components: [f32; 3],
    anchor: SceneObjectAnchor,
    structure: &CrystalStructure,
) -> Vec3 {
    let vector = Vec3::from_array(components);
    match anchor {
        SceneObjectAnchor::World => vector,
        SceneObjectAnchor::Crystal | SceneObjectAnchor::Atom { .. } => {
            crystal_to_world(structure.cell.fractional_to_cartesian(vector))
        }
    }
}

fn object_box_edges(size: [f32; 3]) -> Vec<(Vec3, Vec3)> {
    let half = Vec3::from_array(size) * 0.5;
    let [x, y, z] = [half.x, half.y, half.z];
    let corners = [
        Vec3::new(-x, -y, -z),
        Vec3::new(x, -y, -z),
        Vec3::new(-x, y, -z),
        Vec3::new(x, y, -z),
        Vec3::new(-x, -y, z),
        Vec3::new(x, -y, z),
        Vec3::new(-x, y, z),
        Vec3::new(x, y, z),
    ];
    [
        (0, 1),
        (0, 2),
        (1, 3),
        (2, 3),
        (4, 5),
        (4, 6),
        (5, 7),
        (6, 7),
        (0, 4),
        (1, 5),
        (2, 6),
        (3, 7),
    ]
    .into_iter()
    .map(|(from, to)| (corners[from], corners[to]))
    .collect()
}

/// Number of positions of a site in a supercell, including shared cell-face,
/// edge and corner images. A site at zero appears at every lattice boundary
/// along that axis, not only on the global outer corner.
pub(crate) fn periodic_image_count(fractional: Vec3, supercell: [u32; 3]) -> u64 {
    let fractional = canonical_fractional(fractional);
    [fractional.x, fractional.y, fractional.z]
        .into_iter()
        .zip(supercell)
        .map(|(component, cells)| (cells as u64) + u64::from(component.abs() <= BOUNDARY_EPSILON))
        .product()
}

/// Canonicalize a fractional coordinate before comparing it with a periodic
/// boundary. Symmetry multiplication frequently produces e.g. 0.99999994 for
/// a site that is mathematically at 1 = 0; without this step its image on the
/// outer supercell face would be omitted.
fn canonical_fractional(fractional: Vec3) -> Vec3 {
    Vec3::new(
        canonical_fractional_component(fractional.x),
        canonical_fractional_component(fractional.y),
        canonical_fractional_component(fractional.z),
    )
}

fn canonical_fractional_component(value: f32) -> f32 {
    let wrapped = value.rem_euclid(1.0);
    if wrapped <= BOUNDARY_EPSILON || 1.0 - wrapped <= BOUNDARY_EPSILON {
        0.0
    } else {
        wrapped
    }
}

fn periodic_image_offsets(fractional: Vec3, supercell: [u32; 3]) -> Vec<Vec3> {
    let fractional = canonical_fractional(fractional);
    let ends = [fractional.x, fractional.y, fractional.z]
        .into_iter()
        .zip(supercell)
        .map(|(component, cells)| cells - u32::from(component.abs() > BOUNDARY_EPSILON))
        .collect::<Vec<_>>();
    let mut offsets = Vec::new();
    for ix in 0..=ends[0] {
        for iy in 0..=ends[1] {
            for iz in 0..=ends[2] {
                offsets.push(Vec3::new(ix as f32, iy as f32, iz as f32));
            }
        }
    }
    offsets
}

fn interior_cell_offsets(supercell: [u32; 3]) -> Vec<Vec3> {
    let mut offsets = Vec::new();
    for ix in 0..supercell[0] {
        for iy in 0..supercell[1] {
            for iz in 0..supercell[2] {
                offsets.push(Vec3::new(ix as f32, iy as f32, iz as f32));
            }
        }
    }
    offsets
}

fn cell_edge_key(first: Vec3, second: Vec3) -> ([i64; 3], [i64; 3]) {
    let quantize = |value: Vec3| {
        [
            (value.x * 10_000.0).round() as i64,
            (value.y * 10_000.0).round() as i64,
            (value.z * 10_000.0).round() as i64,
        ]
    };
    let first = quantize(first);
    let second = quantize(second);
    if first <= second {
        (first, second)
    } else {
        (second, first)
    }
}

fn resolved_polyhedron_color(
    rule: &BondRule,
    center_atom: &RenderAtom,
    overrides: &ElementOverrides,
    scheme: elements::ElementColorScheme,
) -> [u8; 3] {
    match rule.polyhedron_color_mode {
        PolyhedronColorMode::A1Color => resolved_style(&center_atom.element, overrides, scheme)
            .color
            .to_srgba()
            .to_u8_array_no_alpha(),
        PolyhedronColorMode::Custom => rule.polyhedron_color,
    }
}

fn polyhedron_material(color: [u8; 3], opacity: f32) -> StandardMaterial {
    StandardMaterial {
        base_color: Color::srgba(
            color[0] as f32 / 255.0,
            color[1] as f32 / 255.0,
            color[2] as f32 / 255.0,
            opacity,
        ),
        alpha_mode: AlphaMode::Blend,
        metallic: 0.08,
        perceptual_roughness: 0.28,
        cull_mode: None,
        ..default()
    }
}

/// Group atoms at one rendered crystallographic position. A CIF represents a
/// mixed site as multiple atom records at the same coordinates; drawing those
/// records as ordinary spheres would make them z-fight and hide all but one
/// component.
fn render_atom_site_groups<'a>(
    atoms: &'a [RenderAtom],
    indices: &[usize],
) -> Vec<Vec<&'a RenderAtom>> {
    let mut group_indices: HashMap<[i64; 3], usize> = HashMap::new();
    let mut groups: Vec<Vec<&RenderAtom>> = Vec::new();
    for &index in indices {
        let atom = &atoms[index];
        let key = [
            (atom.position.x * 10_000.0).round() as i64,
            (atom.position.y * 10_000.0).round() as i64,
            (atom.position.z * 10_000.0).round() as i64,
        ];
        let group = *group_indices.entry(key).or_insert_with(|| {
            groups.push(Vec::new());
            groups.len() - 1
        });
        groups[group].push(atom);
    }
    groups
}

#[allow(clippy::too_many_arguments)]
fn spawn_render_atom_site(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    sphere_mesh: &Handle<Mesh>,
    selection_material: &Handle<StandardMaterial>,
    outline_material: &Handle<StandardMaterial>,
    wireframe_fill_material: &Handle<StandardMaterial>,
    atom_materials: &mut HashMap<String, Handle<StandardMaterial>>,
    atoms: &[&RenderAtom],
    overrides: &ElementOverrides,
    settings: &VisualSettings,
    selection: &SelectionState,
) {
    let Some(first) = atoms.first() else {
        return;
    };
    let uses_sectors = atoms.len() > 1 || first.occupancy < 0.999;
    let mut radius = atoms
        .iter()
        .map(|atom| {
            resolved_style(&atom.element, overrides, settings.element_color_scheme).display_radius
        })
        .fold(0.0_f32, f32::max)
        * settings.atom_scale;
    if settings.atom_display_mode == AtomDisplayMode::SpaceFilling {
        radius *= 1.55;
    }
    if !uses_sectors {
        radius *= first.occupancy.max(0.15).cbrt();
    }
    let selected = atoms.iter().any(|atom| {
        selection
            .atoms
            .iter()
            .any(|selected| selected.matches(atom))
    });
    if selected {
        commands.spawn((
            Mesh3d(sphere_mesh.clone()),
            MeshMaterial3d(selection_material.clone()),
            Transform::from_translation(first.position).with_scale(Vec3::splat(radius * 1.18)),
            CrystalVisual,
        ));
    }
    let total_occupancy: f32 = atoms.iter().map(|atom| atom.occupancy.max(0.0)).sum();
    let occupancy_scale = total_occupancy.max(1.0).recip();
    let occupied_span = (total_occupancy * occupancy_scale).min(1.0) * std::f32::consts::TAU;
    // Keep the partial fan centred at a stable angle in the camera-facing
    // plane; local +Z itself is the axis directed toward the camera.
    let mut start = std::f32::consts::FRAC_PI_2 - occupied_span * 0.5;
    for atom in atoms {
        let fraction = (atom.occupancy.max(0.0) * occupancy_scale).min(1.0);
        if fraction <= 1.0e-5 {
            continue;
        }
        let style = resolved_style(&atom.element, overrides, settings.element_color_scheme);
        let material = atom_materials
            .entry(atom.element.clone())
            .or_insert_with(|| create_atom_material(materials, style, settings))
            .clone();
        let mesh = if uses_sectors {
            meshes.add(occupancy_sector_mesh(
                start,
                start + fraction * std::f32::consts::TAU,
            ))
        } else {
            sphere_mesh.clone()
        };
        if settings.show_outlines && settings.atom_display_mode != AtomDisplayMode::Wireframe {
            let mut outline = commands.spawn((
                Mesh3d(mesh.clone()),
                MeshMaterial3d(outline_material.clone()),
                Transform::from_translation(atom.position).with_scale(Vec3::splat(radius * 1.025)),
                CrystalVisual,
            ));
            if uses_sectors {
                outline.insert(OccupancySector);
            }
        }
        let mut entity = commands.spawn((
            Name::new(if uses_sectors {
                format!("{} {} occupancy sector", atom.element, atom.label)
            } else {
                format!("{} {}", atom.element, atom.label)
            }),
            Mesh3d(mesh),
            MeshMaterial3d(
                if settings.atom_display_mode == AtomDisplayMode::Wireframe {
                    wireframe_fill_material.clone()
                } else {
                    material
                },
            ),
            Transform::from_translation(atom.position).with_scale(Vec3::splat(radius)),
            AtomPickable {
                atom: (*atom).clone(),
                radius,
            },
            CrystalVisual,
        ));
        if settings.atom_display_mode == AtomDisplayMode::Wireframe {
            entity.insert((Wireframe, WireframeColor { color: style.color }));
        }
        if uses_sectors {
            entity.insert(OccupancySector);
        }
        start += fraction * std::f32::consts::TAU;
    }
    let vacancy_fraction = (1.0 - total_occupancy).max(0.0);
    if uses_sectors && vacancy_fraction > 1.0e-5 {
        let vacancy_mesh = meshes.add(occupancy_sector_mesh(
            start,
            start + vacancy_fraction * std::f32::consts::TAU,
        ));
        let vacancy_material = atom_materials
            .entry("__occupancy_vacancy__".to_string())
            .or_insert_with(|| {
                create_atom_material(
                    materials,
                    ElementStyle {
                        color: Color::WHITE,
                        display_radius: 0.0,
                        covalent_radius: 0.0,
                    },
                    settings,
                )
            })
            .clone();
        if settings.show_outlines && settings.atom_display_mode != AtomDisplayMode::Wireframe {
            commands.spawn((
                Mesh3d(vacancy_mesh.clone()),
                MeshMaterial3d(outline_material.clone()),
                Transform::from_translation(first.position).with_scale(Vec3::splat(radius * 1.025)),
                OccupancySector,
                CrystalVisual,
            ));
        }
        let mut vacancy = commands.spawn((
            Name::new(format!("{} vacancy occupancy sector", first.label)),
            Mesh3d(vacancy_mesh),
            MeshMaterial3d(
                if settings.atom_display_mode == AtomDisplayMode::Wireframe {
                    wireframe_fill_material.clone()
                } else {
                    vacancy_material
                },
            ),
            Transform::from_translation(first.position).with_scale(Vec3::splat(radius)),
            OccupancySector,
            CrystalVisual,
        ));
        if settings.atom_display_mode == AtomDisplayMode::Wireframe {
            vacancy.insert((
                Wireframe,
                WireframeColor {
                    color: Color::WHITE,
                },
            ));
        }
    }
}

/// Unit-sphere surface covering an azimuthal occupancy fraction around its
/// local Z axis. With +Z pointed at the camera, the sectors form a pie fan in
/// screen space rather than vertical longitude strips.
fn occupancy_sector_mesh(start: f32, end: f32) -> Mesh {
    const LATITUDE_SEGMENTS: usize = 20;
    const FULL_AZIMUTH_SEGMENTS: f32 = 32.0;
    let span = (end - start).clamp(0.0, std::f32::consts::TAU);
    let azimuth_segments = ((span / std::f32::consts::TAU) * FULL_AZIMUTH_SEGMENTS)
        .ceil()
        .max(2.0) as usize;
    let mut positions = Vec::with_capacity((LATITUDE_SEGMENTS + 1) * (azimuth_segments + 1));
    let mut normals = Vec::with_capacity(positions.capacity());
    for latitude in 0..=LATITUDE_SEGMENTS {
        let theta = std::f32::consts::PI * latitude as f32 / LATITUDE_SEGMENTS as f32;
        let z = theta.cos();
        let radial = theta.sin();
        for azimuth in 0..=azimuth_segments {
            let phi = start + span * azimuth as f32 / azimuth_segments as f32;
            let point = Vec3::new(radial * phi.cos(), radial * phi.sin(), z);
            positions.push(point.to_array());
            normals.push(point.to_array());
        }
    }
    let stride = azimuth_segments + 1;
    let mut indices = Vec::with_capacity(LATITUDE_SEGMENTS * azimuth_segments * 6);
    for latitude in 0..LATITUDE_SEGMENTS {
        for azimuth in 0..azimuth_segments {
            let top_left = (latitude * stride + azimuth) as u32;
            let bottom_left = ((latitude + 1) * stride + azimuth) as u32;
            indices.extend([
                top_left,
                bottom_left,
                top_left + 1,
                top_left + 1,
                bottom_left,
                bottom_left + 1,
            ]);
        }
    }
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_indices(Indices::U32(indices))
}

fn make_render_atom(
    atom: &AtomSite,
    fractional: Vec3,
    cell: &UnitCell,
    center: Vec3,
) -> RenderAtom {
    let position = cell.fractional_to_cartesian(fractional);
    RenderAtom {
        site_id: atom.site_id,
        position: crystal_to_world(position) - center,
        element: atom.element.clone(),
        label: atom.label.clone(),
        fractional,
        symmetry_rotation: atom.symmetry_rotation,
        symmetry_time_reversal: atom.symmetry_time_reversal,
        occupancy: atom.occupancy,
        anisotropic_u: atom.anisotropic_u,
    }
}

fn vector_matches_atom(target: &VectorTarget, atom: &RenderAtom) -> bool {
    match target {
        VectorTarget::Site { site_id } => atom.site_id == *site_id,
        VectorTarget::IndividualAtom {
            site_id,
            fractional,
        } => atom.site_id == *site_id && atom.fractional.distance_squared(*fractional) < 1.0e-8,
    }
}

fn resolved_vector_direction(vector: &VectorLayer, atom: &RenderAtom, cell: &UnitCell) -> Vec3 {
    let source = Vec3::from_array(vector.components);
    let applies_site_symmetry = matches!(&vector.target, VectorTarget::Site { .. });
    let axial_factor = match vector.kind {
        VectorKind::Axial => {
            atom.symmetry_rotation.determinant().signum() * atom.symmetry_time_reversal
        }
        VectorKind::Polar => 1.0,
    };
    match vector.coordinates {
        VectorCoordinates::Lattice => {
            let transformed = if applies_site_symmetry {
                atom.symmetry_rotation * source * axial_factor
            } else {
                source
            };
            crystal_to_world(cell.fractional_to_cartesian(transformed))
        }
        VectorCoordinates::Cartesian => {
            let transformed = if applies_site_symmetry {
                cell.symmetry_to_cartesian(atom.symmetry_rotation) * source * axial_factor
            } else {
                source
            };
            crystal_to_world(transformed)
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn spawn_atom_vector(
    commands: &mut Commands,
    shaft_mesh: Handle<Mesh>,
    head_mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
    origin: Vec3,
    vector: Vec3,
    atom_radius: f32,
    style: &VectorStyle,
    display_scale: f32,
) {
    let vector_length = vector.length() * style.scale * display_scale;
    if vector_length < 1.0e-4 {
        return;
    }
    let direction = vector.normalize();
    let head_length = (0.55 * style.head_length).min(vector_length.max(0.01));
    let (start, tip) = if style.penetrate_atom {
        // VESTA draws a penetrating arrow symmetrically about the atom centre:
        // the full arrow is twice the scaled vector modulus. When the atom
        // radius is added, each exposed half remains vector_length long.
        let half_length = vector_length
            + if style.add_atom_radius {
                atom_radius
            } else {
                0.0
            };
        (
            origin - direction * half_length,
            origin + direction * half_length,
        )
    } else {
        let start = origin
            + direction
                * if style.add_atom_radius {
                    atom_radius
                } else {
                    0.0
                };
        (start, start + direction * vector_length)
    };
    let shaft_end = tip - direction * head_length.min(start.distance(tip) * 0.8);
    spawn_cylinder_between(
        commands,
        shaft_mesh,
        material.clone(),
        start,
        shaft_end,
        style.shaft_radius,
    );
    let actual_head_length = tip.distance(shaft_end);
    commands.spawn((
        Mesh3d(head_mesh),
        MeshMaterial3d(material),
        Transform::from_translation(shaft_end + direction * (actual_head_length * 0.5))
            .with_rotation(Quat::from_rotation_arc(Vec3::Y, direction))
            .with_scale(Vec3::new(
                style.head_radius,
                actual_head_length / 0.42,
                style.head_radius,
            )),
        CrystalVisual,
    ));
}

#[derive(Component, Clone)]
pub(crate) struct AtomPickable {
    pub(crate) atom: RenderAtom,
    pub(crate) radius: f32,
}

struct PolyhedronGeometry {
    mesh: Mesh,
    edges: Vec<(Vec3, Vec3)>,
}

fn coordination_polyhedron(neighbors: &[Vec3]) -> Option<PolyhedronGeometry> {
    if neighbors.len() < 4 || neighbors.len() > 24 {
        return None;
    }
    let center = neighbors.iter().copied().sum::<Vec3>() / neighbors.len() as f32;
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut indices = Vec::new();
    let mut edge_indices = HashSet::new();
    let epsilon = 1.0e-4;

    for first in 0..neighbors.len() {
        for second in first + 1..neighbors.len() {
            for third in second + 1..neighbors.len() {
                let a = neighbors[first];
                let mut b = neighbors[second];
                let mut c = neighbors[third];
                let mut normal = (b - a).cross(c - a);
                if normal.length_squared() < epsilon {
                    continue;
                }
                let mut positive = false;
                let mut negative = false;
                for (index, point) in neighbors.iter().enumerate() {
                    if index == first || index == second || index == third {
                        continue;
                    }
                    let side = normal.dot(*point - a);
                    positive |= side > epsilon;
                    negative |= side < -epsilon;
                    if positive && negative {
                        break;
                    }
                }
                if positive && negative || (!positive && !negative) {
                    continue;
                }
                if normal.dot((a + b + c) / 3.0 - center) < 0.0 {
                    std::mem::swap(&mut b, &mut c);
                    normal = -normal;
                }
                let normal = normal.normalize();
                let base = positions.len() as u32;
                positions.extend([a.to_array(), b.to_array(), c.to_array()]);
                normals.extend([normal.to_array(); 3]);
                indices.extend([base, base + 1, base + 2]);
                for (start, end) in [(first, second), (second, third), (third, first)] {
                    edge_indices.insert((start.min(end), start.max(end)));
                }
            }
        }
    }
    if indices.is_empty() {
        return None;
    }
    Some(PolyhedronGeometry {
        mesh: Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_indices(Indices::U32(indices)),
        edges: edge_indices
            .into_iter()
            .map(|(start, end)| (neighbors[start], neighbors[end]))
            .collect(),
    })
}

fn resolved_style(
    symbol: &str,
    overrides: &ElementOverrides,
    scheme: elements::ElementColorScheme,
) -> ElementStyle {
    let base = elements::style_for_scheme(symbol, scheme);
    overrides
        .0
        .get(symbol)
        .map_or(base, |appearance| ElementStyle {
            color: Color::srgb_u8(
                appearance.color[0],
                appearance.color[1],
                appearance.color[2],
            ),
            display_radius: appearance.radius,
            covalent_radius: base.covalent_radius,
        })
}

pub(crate) fn default_appearance(
    symbol: &str,
    scheme: elements::ElementColorScheme,
) -> ElementAppearance {
    let style = elements::style_for_scheme(symbol, scheme);
    ElementAppearance {
        color: style.color.to_srgba().to_u8_array_no_alpha(),
        radius: style.display_radius,
        visible: true,
    }
}

fn create_atom_material(
    materials: &mut Assets<StandardMaterial>,
    style: ElementStyle,
    settings: &VisualSettings,
) -> Handle<StandardMaterial> {
    materials.add(StandardMaterial {
        base_color: style.color,
        emissive: style.color.to_linear() * 0.035,
        metallic: settings.atom_metallic,
        perceptual_roughness: settings.atom_roughness,
        reflectance: settings.atom_reflectance,
        ..default()
    })
}

fn thermal_ellipsoid_material(
    materials: &mut Assets<StandardMaterial>,
    style: ElementStyle,
    settings: &VisualSettings,
) -> Handle<StandardMaterial> {
    let color = style.color.to_srgba();
    materials.add(StandardMaterial {
        base_color: Color::srgba(color.red, color.green, color.blue, 0.46),
        metallic: settings.atom_metallic * 0.25,
        perceptual_roughness: settings.atom_roughness,
        reflectance: settings.atom_reflectance,
        alpha_mode: AlphaMode::Blend,
        cull_mode: None,
        ..default()
    })
}

fn thermal_ellipsoid_transform(
    cell: &UnitCell,
    displacement: [[f32; 3]; 3],
    position: Vec3,
    scale: f32,
) -> Option<Transform> {
    let fractional_u = Mat3::from_cols_array(&[
        displacement[0][0],
        displacement[1][0],
        displacement[2][0],
        displacement[0][1],
        displacement[1][1],
        displacement[2][1],
        displacement[0][2],
        displacement[1][2],
        displacement[2][2],
    ]);
    let basis = Mat3::from_cols(cell.vectors[0], cell.vectors[1], cell.vectors[2]);
    let crystal_u = basis * fractional_u * basis.transpose();
    let world_rotation = Mat3::from_cols(
        crystal_to_world(Vec3::X),
        crystal_to_world(Vec3::Y),
        crystal_to_world(Vec3::Z),
    );
    let world_u = world_rotation * crystal_u * world_rotation.transpose();
    let (values, mut axes) = symmetric_eigen_decomposition(world_u);
    if values.max_element() <= 1.0e-8 {
        return None;
    }
    if axes.determinant() < 0.0 {
        axes.z_axis = -axes.z_axis;
    }
    Some(Transform {
        translation: position,
        rotation: Quat::from_mat3(&axes),
        scale: Vec3::new(
            values.x.max(0.0).sqrt() * scale,
            values.y.max(0.0).sqrt() * scale,
            values.z.max(0.0).sqrt() * scale,
        ),
    })
}

/// Jacobi diagonalization for a real symmetric 3×3 displacement tensor.
fn symmetric_eigen_decomposition(matrix: Mat3) -> (Vec3, Mat3) {
    let mut values: [[f32; 3]; 3] = [
        [matrix.x_axis.x, matrix.y_axis.x, matrix.z_axis.x],
        [matrix.x_axis.y, matrix.y_axis.y, matrix.z_axis.y],
        [matrix.x_axis.z, matrix.y_axis.z, matrix.z_axis.z],
    ];
    let mut vectors = [[0.0; 3]; 3];
    for (index, row) in vectors.iter_mut().enumerate() {
        row[index] = 1.0;
    }
    for _ in 0..16 {
        let ((first, second), magnitude) = [
            ((0, 1), values[0][1].abs()),
            ((0, 2), values[0][2].abs()),
            ((1, 2), values[1][2].abs()),
        ]
        .into_iter()
        .max_by(|left, right| left.1.total_cmp(&right.1))
        .expect("Jacobi candidates are non-empty");
        if magnitude < 1.0e-7 {
            break;
        }
        let angle = 0.5
            * (2.0 * values[first][second]).atan2(values[second][second] - values[first][first]);
        let (sin, cos) = angle.sin_cos();
        for index in 0..3 {
            if index == first || index == second {
                continue;
            }
            let left = values[index][first];
            let right = values[index][second];
            values[index][first] = cos * left - sin * right;
            values[first][index] = values[index][first];
            values[index][second] = sin * left + cos * right;
            values[second][index] = values[index][second];
        }
        let left = values[first][first];
        let right = values[second][second];
        let cross = values[first][second];
        values[first][first] = cos * cos * left - 2.0 * sin * cos * cross + sin * sin * right;
        values[second][second] = sin * sin * left + 2.0 * sin * cos * cross + cos * cos * right;
        values[first][second] = 0.0;
        values[second][first] = 0.0;
        for row in &mut vectors {
            let left = row[first];
            let right = row[second];
            row[first] = cos * left - sin * right;
            row[second] = sin * left + cos * right;
        }
    }
    (
        Vec3::new(values[0][0], values[1][1], values[2][2]),
        Mat3::from_cols(
            Vec3::new(vectors[0][0], vectors[1][0], vectors[2][0]),
            Vec3::new(vectors[0][1], vectors[1][1], vectors[2][1]),
            Vec3::new(vectors[0][2], vectors[1][2], vectors[2][2]),
        ),
    )
}

fn spawn_cylinder_between(
    commands: &mut Commands,
    mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
    start: Vec3,
    end: Vec3,
    radius: f32,
) {
    let direction = end - start;
    let length = direction.length();
    if length < 1.0e-5 {
        return;
    }
    commands.spawn((
        Mesh3d(mesh),
        MeshMaterial3d(material),
        Transform::from_translation((start + end) * 0.5)
            .with_rotation(Quat::from_rotation_arc(Vec3::Y, direction / length))
            .with_scale(Vec3::new(radius, length, radius)),
        CrystalVisual,
    ));
}

#[allow(clippy::too_many_arguments)]
fn spawn_cylinder_bond(
    commands: &mut Commands,
    materials: &mut Assets<StandardMaterial>,
    cylinder_mesh: &Handle<Mesh>,
    first_material: Handle<StandardMaterial>,
    second_material: Handle<StandardMaterial>,
    start: Vec3,
    end: Vec3,
    rule: &BondRule,
) {
    let solid_material = |color: [u8; 3], materials: &mut Assets<StandardMaterial>| {
        materials.add(StandardMaterial {
            base_color: Color::srgb_u8(color[0], color[1], color[2]),
            emissive: Color::srgb_u8(color[0], color[1], color[2]).to_linear() * 0.04,
            perceptual_roughness: 0.32,
            ..default()
        })
    };
    match rule.bond_style {
        BondStyle::UnicolorCylinder => {
            spawn_cylinder_between(
                commands,
                cylinder_mesh.clone(),
                solid_material(rule.bond_color, materials),
                start,
                end,
                rule.bond_radius,
            );
        }
        BondStyle::BicolorCylinder => {
            let midpoint = (start + end) * 0.5;
            spawn_cylinder_between(
                commands,
                cylinder_mesh.clone(),
                first_material,
                start,
                midpoint,
                rule.bond_radius,
            );
            spawn_cylinder_between(
                commands,
                cylinder_mesh.clone(),
                second_material,
                midpoint,
                end,
                rule.bond_radius,
            );
        }
        BondStyle::ColorLine
        | BondStyle::GradientLine
        | BondStyle::DottedLine
        | BondStyle::DashedLine => unreachable!("line styles are rendered as retained gizmos"),
    }
}

fn bond_gizmo_line_style(style: BondStyle) -> Option<GizmoLineStyle> {
    match style {
        BondStyle::ColorLine | BondStyle::GradientLine => Some(GizmoLineStyle::Solid),
        BondStyle::DottedLine => Some(GizmoLineStyle::Dotted),
        BondStyle::DashedLine => Some(GizmoLineStyle::Dashed {
            gap_scale: 0.60,
            line_scale: 1.00,
        }),
        BondStyle::UnicolorCylinder | BondStyle::BicolorCylinder => None,
    }
}

pub(crate) fn atoms_are_bonded(
    first: &RenderAtom,
    second: &RenderAtom,
    settings: &VisualSettings,
) -> bool {
    let supercell = settings.supercell.map(|cells| cells.max(1) as f32);
    settings.bond_rules.iter().any(|rule| {
        rule.enabled
            && (rule_matches_with_boundary(first, second, rule, settings, supercell)
                || rule_matches_with_boundary(second, first, rule, settings, supercell))
    })
}

pub(crate) fn rule_matches_directed(
    center: &RenderAtom,
    neighbor: &RenderAtom,
    rule: &BondRule,
) -> bool {
    atom_matches(&rule.center, center, rule.match_mode)
        && center.position.distance(neighbor.position) > 1.0e-4
        && center.position.distance(neighbor.position) >= rule.min_length.max(0.0)
        && center.position.distance(neighbor.position) <= rule.max_length.max(0.0)
        && match rule.search_mode {
            BondSearchMode::A2BondedToA1 => atom_matches(&rule.neighbor, neighbor, rule.match_mode),
            // VESTA's molecule search needs a persistent molecular graph.
            // Until such a graph exists, retain all candidate neighbors rather
            // than silently dropping a valid imported rule.
            BondSearchMode::AtomsBondedToA1 | BondSearchMode::Molecules => true,
        }
}

fn atom_matches(target: &str, atom: &RenderAtom, match_mode: BondMatchMode) -> bool {
    let target = target.trim();
    if target.is_empty() || target == "*" || target.eq_ignore_ascii_case("any") {
        return true;
    }
    let value = match match_mode {
        BondMatchMode::Element => &atom.element,
        BondMatchMode::SiteLabel => &atom.label,
    };
    value.eq_ignore_ascii_case(target)
}

fn find_bonds(atoms: &[RenderAtom], settings: &VisualSettings) -> Vec<(usize, usize)> {
    if atoms.len() < 2 || !settings.bond_rules.iter().any(|rule| rule.enabled) {
        return Vec::new();
    }
    let maximum_cutoff = settings
        .bond_rules
        .iter()
        .filter(|rule| rule.enabled)
        .map(|rule| rule.max_length)
        .fold(0.5, f32::max);
    let bucket_size = maximum_cutoff.max(0.5);
    let key = |p: Vec3| {
        (
            (p.x / bucket_size).floor() as i32,
            (p.y / bucket_size).floor() as i32,
            (p.z / bucket_size).floor() as i32,
        )
    };
    let mut buckets: HashMap<(i32, i32, i32), Vec<usize>> = HashMap::new();
    for (index, atom) in atoms.iter().enumerate() {
        buckets.entry(key(atom.position)).or_default().push(index);
    }
    let mut bonds = HashSet::new();
    let supercell = settings.supercell.map(|cells| cells.max(1) as f32);
    for (index, atom) in atoms.iter().enumerate() {
        let (bx, by, bz) = key(atom.position);
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    if let Some(candidates) = buckets.get(&(bx + dx, by + dy, bz + dz)) {
                        for &other_index in candidates {
                            if other_index <= index {
                                continue;
                            }
                            let other = &atoms[other_index];
                            if settings.bond_rules.iter().any(|rule| {
                                rule.enabled
                                    && (rule_matches_with_boundary(
                                        atom, other, rule, settings, supercell,
                                    ) || rule_matches_with_boundary(
                                        other, atom, rule, settings, supercell,
                                    ))
                            }) {
                                bonds.insert((index, other_index));
                            }
                        }
                    }
                }
            }
        }
    }
    let mut result: Vec<_> = bonds.into_iter().collect();
    result.sort_unstable();
    result
}

fn rule_matches_with_boundary(
    center: &RenderAtom,
    neighbor: &RenderAtom,
    rule: &BondRule,
    _settings: &VisualSettings,
    supercell: [f32; 3],
) -> bool {
    rule_matches_directed(center, neighbor, rule)
        && match rule.boundary_mode {
            BondBoundaryMode::DoNotSearchBeyond => {
                atom_is_inside_supercell(center, supercell)
                    && atom_is_inside_supercell(neighbor, supercell)
            }
            BondBoundaryMode::SearchAdditionalForA1 => atom_is_inside_supercell(center, supercell),
            BondBoundaryMode::SearchRecursivelyIfEitherVisible => {
                atom_is_inside_supercell(center, supercell)
                    || atom_is_inside_supercell(neighbor, supercell)
            }
        }
}

fn bond_is_displayed(first: &RenderAtom, second: &RenderAtom, settings: &VisualSettings) -> bool {
    displayed_bond_rule(first, second, settings).is_some()
}

fn displayed_bond_rule<'a>(
    first: &RenderAtom,
    second: &RenderAtom,
    settings: &'a VisualSettings,
) -> Option<(usize, &'a BondRule)> {
    let supercell = settings.supercell.map(|cells| cells.max(1) as f32);
    let matches_rule = |rule: &BondRule| {
        rule.enabled
            && (rule_matches_with_boundary(first, second, rule, settings, supercell)
                || rule_matches_with_boundary(second, first, rule, settings, supercell))
    };
    // A coordination polyhedron replaces every bond sharing its rule. This
    // avoids opaque bond segments obscuring its faces and edges.
    if settings
        .bond_rules
        .iter()
        .any(|rule| rule.show_polyhedra && matches_rule(rule))
    {
        return None;
    }
    settings
        .bond_rules
        .iter()
        .enumerate()
        .find(|(_, rule)| matches_rule(rule))
}

fn bond_supports_polyhedron(
    first: &RenderAtom,
    second: &RenderAtom,
    settings: &VisualSettings,
) -> bool {
    let supercell = settings.supercell.map(|cells| cells.max(1) as f32);
    settings.bond_rules.iter().any(|rule| {
        rule.enabled
            && rule.show_polyhedra
            && (rule_matches_with_boundary(first, second, rule, settings, supercell)
                || rule_matches_with_boundary(second, first, rule, settings, supercell))
    })
}

fn atom_is_inside_supercell(atom: &RenderAtom, supercell: [f32; 3]) -> bool {
    const EPSILON: f32 = 1.0e-4;
    let fractional = atom.fractional;
    fractional.x >= -EPSILON
        && fractional.y >= -EPSILON
        && fractional.z >= -EPSILON
        // The drawing boundary is closed: a periodic copy exactly on an
        // outer cell face is a valid A1 center. Only images strictly beyond
        // that face are additional A2 candidates.
        && fractional.x <= supercell[0] + EPSILON
        && fractional.y <= supercell[1] + EPSILON
        && fractional.z <= supercell[2] + EPSILON
}

fn cell_edges([a, b, c]: [Vec3; 3]) -> Vec<(Vec3, Vec3)> {
    vec![
        (Vec3::ZERO, a),
        (Vec3::ZERO, b),
        (Vec3::ZERO, c),
        (a, a + b),
        (a, a + c),
        (b, a + b),
        (b, b + c),
        (c, a + c),
        (c, b + c),
        (a + b, a + b + c),
        (a + c, a + b + c),
        (b + c, a + b + c),
    ]
}

pub(crate) fn crystal_to_world(value: Vec3) -> Vec3 {
    // Keep c visually upright (world Y) while preserving the crystallographic
    // handedness: a × b = c. Swapping Y/Z alone would reflect the structure;
    // the minus sign maps b toward world -Z and makes this a proper rotation.
    Vec3::new(value.x, value.z, -value.y)
}

pub(crate) fn view_directions(structure: &CrystalStructure) -> [(&'static str, Vec3); 7] {
    let a = crystal_to_world(structure.cell.vectors[0]);
    let b = crystal_to_world(structure.cell.vectors[1]);
    let c = crystal_to_world(structure.cell.vectors[2]);
    [
        ("a", a),
        ("b", b),
        ("c", c),
        ("a*", b.cross(c)),
        ("b*", c.cross(a)),
        ("c*", a.cross(b)),
        ("ISO", a.normalize() + b.normalize() + c.normalize()),
    ]
}

#[cfg(test)]
mod tests {
    use super::{
        canonical_fractional, crystal_to_world, miller_plane_normal, occupancy_sector_rotation,
        occupancy_sector_rotation_for_view, periodic_image_count, periodic_image_offsets,
        resolved_vector_direction, vector_matches_atom,
    };
    use bevy::prelude::{EulerRot, Mat3, Quat, Vec3};
    use wyckoscope::model::{SiteId, UnitCell};

    use crate::state::{
        RenderAtom, VectorCoordinates, VectorKind, VectorLayer, VectorStyle, VectorTarget,
    };

    #[test]
    fn fills_all_shared_boundaries_of_a_supercell() {
        assert_eq!(
            periodic_image_count(Vec3::new(0.25, 0.5, 0.75), [2, 2, 1]),
            4
        );
        assert_eq!(periodic_image_count(Vec3::ZERO, [2, 2, 1]), 18);
        assert_eq!(
            periodic_image_count(Vec3::new(0.999_999, 0.0, 0.0), [2, 2, 1]),
            18
        );

        let offsets = periodic_image_offsets(Vec3::ZERO, [2, 2, 1]);
        assert_eq!(offsets.len(), 18);
        assert!(offsets.contains(&Vec3::new(1.0, 1.0, 1.0)));
        assert!(offsets.contains(&Vec3::new(2.0, 1.0, 1.0)));
        assert_eq!(
            canonical_fractional(Vec3::new(0.999_999, 0.0, -0.000_001)),
            Vec3::ZERO
        );
    }

    #[test]
    fn crystal_to_world_preserves_right_handed_axes() {
        let a = crystal_to_world(Vec3::X);
        let b = crystal_to_world(Vec3::Y);
        let c = crystal_to_world(Vec3::Z);
        assert!(a.cross(b).dot(c) > 0.999);
    }

    #[test]
    fn occupancy_sector_faces_the_camera_from_each_site() {
        let camera = Vec3::new(4.0, 3.0, 10.0);
        let site = Vec3::new(-2.0, 1.0, -1.0);
        let rotation = occupancy_sector_rotation(camera, Vec3::Y, site).expect("distinct points");
        let expected = (camera - site).normalize();
        assert!((rotation * Vec3::Z).dot(expected) > 0.999_9);
    }

    #[test]
    fn occupancy_sector_orthographic_view_uses_a_shared_camera_direction() {
        let camera_rotation = Quat::from_euler(EulerRot::YXZ, 0.7, -0.35, 0.0);
        let toward_camera = camera_rotation * Vec3::Z;
        let camera_up = camera_rotation * Vec3::Y;
        let rotation = occupancy_sector_rotation_for_view(toward_camera, camera_up)
            .expect("valid view direction");
        assert!((rotation * Vec3::Z).dot(toward_camera) > 0.999_9);
    }

    #[test]
    fn miller_plane_uses_the_reciprocal_lattice_without_two_pi() {
        let cell = UnitCell::new(2.0, 3.0, 4.0, 90.0, 90.0, 90.0);
        let (normal, offset) = miller_plane_normal(&cell, [1, 0, 0], 1.0).expect("valid plane");
        assert!(normal.distance(Vec3::X) < 1.0e-5);
        assert!(offset.distance(Vec3::new(2.0, 0.0, 0.0)) < 1.0e-5);

        let (normal, offset) = miller_plane_normal(&cell, [0, 0, 1], 1.0).expect("valid plane");
        assert!(normal.distance(Vec3::Y) < 1.0e-5);
        assert!(offset.distance(Vec3::new(0.0, 4.0, 0.0)) < 1.0e-5);
    }

    #[test]
    fn axial_site_vectors_apply_magnetic_time_reversal_but_polar_vectors_do_not() {
        let cell = UnitCell::new(1.0, 1.0, 1.0, 90.0, 90.0, 90.0);
        let atom = RenderAtom {
            site_id: SiteId(7),
            position: Vec3::ZERO,
            element: "Fe".to_string(),
            label: "Fe1".to_string(),
            fractional: Vec3::ZERO,
            symmetry_rotation: Mat3::IDENTITY,
            symmetry_time_reversal: -1.0,
            occupancy: 1.0,
            anisotropic_u: None,
        };
        let mut vector = VectorLayer {
            enabled: true,
            target: VectorTarget::Site { site_id: SiteId(7) },
            kind: VectorKind::Axial,
            components: [0.0, 0.0, 1.0],
            coordinates: VectorCoordinates::Cartesian,
            style: VectorStyle {
                color: [255, 0, 0],
                scale: 1.0,
                shaft_radius: 0.1,
                head_length: 1.0,
                head_radius: 1.0,
                penetrate_atom: true,
                add_atom_radius: false,
            },
        };
        assert_eq!(
            resolved_vector_direction(&vector, &atom, &cell),
            crystal_to_world(-Vec3::Z)
        );
        vector.kind = VectorKind::Polar;
        assert_eq!(
            resolved_vector_direction(&vector, &atom, &cell),
            crystal_to_world(Vec3::Z)
        );
    }

    #[test]
    fn individual_vector_target_survives_world_recentering() {
        let target = VectorTarget::IndividualAtom {
            site_id: SiteId(12),
            fractional: Vec3::new(1.0, 0.5, 0.0),
        };
        let atom = RenderAtom {
            site_id: SiteId(12),
            position: Vec3::new(-3.0, 7.0, 2.0),
            element: "O".to_string(),
            label: "O1".to_string(),
            fractional: Vec3::new(1.0, 0.5, 0.0),
            symmetry_rotation: Mat3::IDENTITY,
            symmetry_time_reversal: 1.0,
            occupancy: 1.0,
            anisotropic_u: None,
        };
        assert!(vector_matches_atom(&target, &atom));

        let recentered_atom = RenderAtom {
            position: Vec3::new(14.0, -8.0, -5.0),
            ..atom
        };
        assert!(vector_matches_atom(&target, &recentered_atom));
    }
}
