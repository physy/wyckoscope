use bevy::{
    camera::{ScalingMode, Viewport},
    input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll},
    prelude::*,
};
use bevy_egui::input::EguiWantsInput;

use crate::{
    RENDER_TAB_BAR_HEIGHT,
    scene::{AtomPickable, atoms_are_bonded, crystal_to_world},
    state::*,
};

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(crate) fn select_atom(
    mut pointer: ResMut<SelectionPointer>,
    mut selection: ResMut<SelectionState>,
    mut dirty: ResMut<SceneDirty>,
    window: Single<&Window>,
    camera: Single<
        (&Camera, &GlobalTransform),
        (
            With<OrbitCamera>,
            Without<BackgroundCamera>,
            Without<OrientationAxisCamera>,
        ),
    >,
    atoms: Query<&AtomPickable>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    egui_input: Res<EguiWantsInput>,
    layout: Res<InspectorLayout>,
    settings: Res<VisualSettings>,
) {
    if mouse_buttons.just_pressed(MouseButton::Left) {
        pointer.press_position = (!egui_input.wants_any_pointer_input())
            .then(|| window.cursor_position())
            .flatten()
            .filter(|position| position.x >= layout.width && position.y >= RENDER_TAB_BAR_HEIGHT);
    }
    if !mouse_buttons.just_released(MouseButton::Left) {
        return;
    }
    let Some(press_position) = pointer.press_position.take() else {
        return;
    };
    let Some(cursor_position) = window.cursor_position() else {
        return;
    };
    if cursor_position.distance(press_position) > 5.0 {
        return;
    }
    let (camera, camera_transform) = *camera;
    let picked = camera
        .viewport_to_world(camera_transform, cursor_position)
        .ok()
        .and_then(|ray| {
            atoms
                .iter()
                .filter_map(|pickable| {
                    let to_center = pickable.atom.position - ray.origin;
                    let along_ray = to_center.dot(*ray.direction);
                    if along_ray <= 0.0 {
                        return None;
                    }
                    let closest_distance_sq = to_center.length_squared() - along_ray * along_ray;
                    let radius_sq = pickable.radius * pickable.radius;
                    if closest_distance_sq > radius_sq {
                        return None;
                    }
                    Some((
                        along_ray - (radius_sq - closest_distance_sq).sqrt(),
                        pickable,
                    ))
                })
                .min_by(|(left, _), (right, _)| left.total_cmp(right))
                .map(|(_, pickable)| pickable)
        });
    let shift = keyboard.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    if let Some(picked) = picked {
        let clicked_atom = picked.atom.clone();
        let coordination = atoms
            .iter()
            .filter(|other| atoms_are_bonded(&picked.atom, &other.atom, &settings))
            .count();
        let picked = SelectedAtom {
            site_id: picked.atom.site_id,
            element: picked.atom.element.clone(),
            label: picked.atom.label.clone(),
            position: picked.atom.position,
            fractional: picked.atom.fractional,
            coordination,
        };
        if shift {
            if let Some(index) = selection
                .atoms
                .iter()
                .position(|selected| selected.matches(&clicked_atom))
            {
                selection.atoms.remove(index);
            } else {
                if selection.atoms.len() == 4 {
                    selection.atoms.remove(0);
                }
                selection.atoms.push(picked);
            }
        } else {
            selection.atoms.clear();
            selection.atoms.push(picked);
        }
        dirty.mark(false);
    } else if !shift && !selection.atoms.is_empty() {
        selection.atoms.clear();
        dirty.mark(false);
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(crate) fn orbit_camera(
    mut camera: Single<(&mut OrbitCamera, &mut Transform, &mut Projection), With<OrbitCamera>>,
    window: Single<&Window>,
    layout: Res<InspectorLayout>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    mouse_motion: Res<AccumulatedMouseMotion>,
    mouse_scroll: Res<AccumulatedMouseScroll>,
    keyboard: Res<ButtonInput<KeyCode>>,
    egui_input: Res<EguiWantsInput>,
    settings: Res<VisualSettings>,
    time: Res<Time>,
) {
    let (orbit, transform, projection) = &mut *camera;
    if keyboard.just_pressed(KeyCode::KeyF) {
        orbit.reset();
    }
    if !egui_input.wants_any_pointer_input() {
        let delta = mouse_motion.delta;
        let shift = keyboard.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
        let pan = mouse_buttons.pressed(MouseButton::Middle)
            || mouse_buttons.pressed(MouseButton::Right)
            || (shift && mouse_buttons.pressed(MouseButton::Left));
        if mouse_buttons.pressed(MouseButton::Left) && !pan {
            if let Some(current) = window.cursor_position().and_then(|position| {
                arcball_position(position, &window, layout.width, RENDER_TAB_BAR_HEIGHT)
            }) {
                if let Some(previous) = orbit.arcball_anchor {
                    let incremental = Quat::from_rotation_arc(current, previous);
                    orbit.rotation = (orbit.rotation * incremental).normalize();
                }
                orbit.arcball_anchor = Some(current);
            }
        } else if pan {
            orbit.arcball_anchor = None;
            let right = transform.rotation * Vec3::X;
            let up = transform.rotation * Vec3::Y;
            let pan_scale = orbit.radius * 0.0014;
            orbit.target += (-right * delta.x + up * delta.y) * pan_scale;
        } else {
            orbit.arcball_anchor = None;
        }
        if mouse_scroll.delta.y.abs() > f32::EPSILON {
            orbit.radius *= (-mouse_scroll.delta.y * 0.003).exp();
            orbit.radius = orbit.radius.clamp(1.5, 900.0);
        }
    }
    if settings.auto_rotate && !mouse_buttons.any_pressed([MouseButton::Left, MouseButton::Right]) {
        orbit.rotation =
            (Quat::from_rotation_y(time.delta_secs() * 0.16) * orbit.rotation).normalize();
    }
    let camera_distance =
        apply_projection_blend(projection, orbit.radius, settings.projection_blend);
    update_camera_transform_at_distance(orbit, transform, camera_distance);
}

#[allow(clippy::type_complexity)]
pub(crate) fn update_camera_viewports(
    window: Single<&Window>,
    layout: Res<InspectorLayout>,
    mut main_camera: Single<
        &mut Camera,
        (
            With<OrbitCamera>,
            Without<BackgroundCamera>,
            Without<OrientationAxisCamera>,
            Without<InterfaceCamera>,
        ),
    >,
    mut background_camera: Single<
        &mut Camera,
        (
            With<BackgroundCamera>,
            Without<OrbitCamera>,
            Without<OrientationAxisCamera>,
            Without<InterfaceCamera>,
        ),
    >,
) {
    let physical_size = window.resolution.physical_size();
    // During minimization and some resize transitions winit briefly reports a
    // zero-sized surface. A 1×1 viewport is not valid for that target either,
    // so disable the window cameras until the surface has a real extent again.
    if physical_size.x == 0 || physical_size.y == 0 {
        main_camera.is_active = false;
        main_camera.viewport = None;
        background_camera.is_active = false;
        background_camera.viewport = None;
        return;
    }

    main_camera.is_active = true;
    background_camera.is_active = true;
    let scale_factor = window.scale_factor();
    let panel_width = (layout.width * scale_factor).round() as u32;
    let render_x = panel_width.min(physical_size.x.saturating_sub(1));
    let render_y = (RENDER_TAB_BAR_HEIGHT * scale_factor).round() as u32;
    let render_y = render_y.min(physical_size.y.saturating_sub(1));
    let render_size = UVec2::new(
        physical_size.x.saturating_sub(render_x).max(1),
        physical_size.y.saturating_sub(render_y).max(1),
    );
    let render_viewport = Viewport {
        physical_position: UVec2::new(render_x, render_y),
        physical_size: render_size,
        ..default()
    };
    main_camera.viewport = Some(render_viewport.clone());
    background_camera.viewport = Some(render_viewport);
}

#[allow(clippy::type_complexity)]
pub(crate) fn sync_orientation_axes(
    state: Res<AppState>,
    settings: Res<VisualSettings>,
    axis_materials: Res<OrientationAxisMaterials>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    main_camera: Single<
        &Transform,
        (
            With<OrbitCamera>,
            Without<OrientationAxisCamera>,
            Without<OrientationAxisRoot>,
        ),
    >,
    mut axis_camera: Single<
        &mut Transform,
        (
            With<OrientationAxisCamera>,
            Without<OrbitCamera>,
            Without<OrientationAxisRoot>,
        ),
    >,
    mut roots: Query<
        (&OrientationAxisRoot, &mut Transform),
        (Without<OrbitCamera>, Without<OrientationAxisCamera>),
    >,
) {
    let axes = state.structure.cell.vectors.map(crystal_to_world);
    for (root, mut transform) in &mut roots {
        let direction = axes[root.0].normalize_or_zero();
        transform.rotation = if direction.length_squared() > 0.5 {
            Quat::from_rotation_arc(Vec3::Y, direction)
        } else {
            Quat::IDENTITY
        };
    }

    axis_camera.rotation = main_camera.rotation;
    axis_camera.translation = main_camera.rotation * Vec3::Z * 4.0;

    for (index, handle) in axis_materials.0.iter().enumerate() {
        if let Some(material) = materials.get_mut(handle) {
            let color = Color::srgb_u8(
                settings.axis_colors[index][0],
                settings.axis_colors[index][1],
                settings.axis_colors[index][2],
            );
            material.base_color = color;
            material.emissive = color.to_linear() * 0.08;
        }
    }
}

pub(crate) fn update_camera_transform(orbit: &OrbitCamera, transform: &mut Transform) {
    update_camera_transform_at_distance(orbit, transform, orbit.radius);
}

fn update_camera_transform_at_distance(
    orbit: &OrbitCamera,
    transform: &mut Transform,
    distance: f32,
) {
    transform.translation = orbit.target + orbit.rotation * Vec3::new(0.0, 0.0, distance);
    transform.rotation = orbit.rotation;
}

fn arcball_position(
    cursor: Vec2,
    window: &Window,
    panel_width: f32,
    render_top: f32,
) -> Option<Vec3> {
    let render_width = (window.width() - panel_width).max(1.0);
    let render_height = (window.height() - render_top).max(1.0);
    if cursor.x < panel_width || cursor.y < render_top {
        return None;
    }
    let scale = render_width.min(render_height);
    let point = Vec2::new(
        (2.0 * (cursor.x - panel_width) - render_width) / scale,
        (render_height - 2.0 * (cursor.y - render_top)) / scale,
    );
    let radius_squared = point.length_squared();
    Some(if radius_squared <= 1.0 {
        Vec3::new(point.x, point.y, (1.0 - radius_squared).sqrt())
    } else {
        Vec3::new(point.x, point.y, 0.0).normalize()
    })
}

fn apply_projection_blend(projection: &mut Projection, radius: f32, blend: f32) -> f32 {
    let blend = blend.clamp(-1.0, 1.0);
    let base_half_fov = std::f32::consts::FRAC_PI_8;
    if blend < 0.0 {
        let strength = -blend;
        let eased = strength * strength * (3.0 - 2.0 * strength);
        let half_fov = base_half_fov + (std::f32::consts::FRAC_PI_4 - base_half_fov) * eased;
        *projection = Projection::Perspective(PerspectiveProjection {
            fov: half_fov * 2.0,
            near: 0.02,
            far: 1_000_000.0,
            ..default()
        });
        return radius * base_half_fov.tan() / half_fov.tan();
    }
    if blend >= 0.995 {
        *projection = Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::FixedVertical {
                viewport_height: 2.0 * 18.0 * (std::f32::consts::FRAC_PI_8).tan(),
            },
            scale: (radius / 18.0).clamp(0.08, 50.0),
            ..OrthographicProjection::default_3d()
        });
        return radius;
    }

    // A dolly-zoom continuously removes perspective while preserving the framing.
    // At the slider endpoint we switch to the mathematically equivalent orthographic view.
    let eased = blend * blend * (3.0 - 2.0 * blend);
    let minimum_half_fov = 0.25_f32.to_radians();
    let half_fov = base_half_fov + (minimum_half_fov - base_half_fov) * eased;
    *projection = Projection::Perspective(PerspectiveProjection {
        fov: half_fov * 2.0,
        far: 1_000_000.0,
        ..default()
    });
    radius * base_half_fov.tan() / half_fov.tan()
}
