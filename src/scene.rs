use std::collections::{HashMap, HashSet};

use bevy::{
    asset::RenderAssetUsages,
    camera::{CameraOutputMode, Exposure, ScalingMode, visibility::RenderLayers},
    core_pipeline::tonemapping::Tonemapping,
    mesh::{Indices, PrimitiveTopology},
    pbr::wireframe::{Wireframe, WireframeColor},
    prelude::*,
    render::render_resource::{BlendState, Face},
};
use bevy_egui::PrimaryEguiContext;
use wyckoscope::{
    elements::{self, ElementStyle},
    model::{AtomSite, CrystalStructure},
};

use crate::{AXIS_RENDER_LAYER, MAX_RENDER_ATOMS, camera::update_camera_transform, state::*};

const BOUNDARY_EPSILON: f32 = 1.0e-4;

pub(crate) fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.spawn((
        Name::new("Exact-color background camera"),
        Camera2d,
        Camera {
            order: -2,
            clear_color: ClearColorConfig::Custom(Color::WHITE),
            ..default()
        },
        BackgroundCamera,
    ));

    commands.spawn((
        Name::new("Presentation camera"),
        Camera3d::default(),
        Camera {
            order: 0,
            clear_color: ClearColorConfig::Custom(Color::NONE),
            output_mode: CameraOutputMode::Write {
                blend_state: Some(BlendState::ALPHA_BLENDING),
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

    let axis_colors = [
        Color::srgb_u8(235, 31, 31),
        Color::srgb_u8(31, 184, 51),
        Color::srgb_u8(31, 82, 245),
    ];
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
    commands.insert_resource(OrientationAxisMaterials(axis_materials));
    commands.spawn((
        Name::new("3D orientation-axis camera"),
        Camera3d::default(),
        Camera {
            order: 1,
            clear_color: ClearColorConfig::None,
            output_mode: CameraOutputMode::Write {
                blend_state: Some(BlendState::ALPHA_BLENDING),
                clear_color: ClearColorConfig::None,
            },
            ..default()
        },
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
            shadows_enabled: false,
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
            shadows_enabled: false,
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
            order: 2,
            clear_color: ClearColorConfig::None,
            output_mode: CameraOutputMode::Write {
                blend_state: Some(BlendState::ALPHA_BLENDING),
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
            shadows_enabled: true,
            shadow_depth_bias: 0.02,
            shadow_normal_bias: 0.55,
            ..default()
        },
        Transform::from_xyz(8.0, 10.0, 12.0),
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
            shadows_enabled: true,
            shadow_depth_bias: 0.02,
            shadow_normal_bias: 0.55,
            ..default()
        },
        Transform::from_xyz(8.0, 10.0, 12.0).looking_at(Vec3::ZERO, Vec3::Y),
        Visibility::Hidden,
        LightRole::KeySpot,
    ));
    commands.spawn((
        Name::new("Soft fill"),
        DirectionalLight {
            color: Color::srgb(0.52, 0.66, 1.0),
            illuminance: 4_800.0,
            shadows_enabled: false,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.8, 2.4, 0.0)),
        LightRole::Fill,
    ));
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
    let cell_count = supercell
        .iter()
        .fold(1_u64, |total, value| total.saturating_mul(*value as u64));
    let boundary_atom_count = settings.show_boundary_atoms.then(|| {
        source_atoms
            .iter()
            .map(|atom| boundary_image_count(atom.fractional).saturating_sub(1))
            .sum::<u64>()
    });
    let requested_atoms = cell_count
        .saturating_mul(source_atoms.len() as u64)
        .saturating_add(boundary_atom_count.unwrap_or_default());
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
    let cell_material = materials.add(StandardMaterial {
        base_color: Color::srgb_u8(
            settings.unit_cell_color[0],
            settings.unit_cell_color[1],
            settings.unit_cell_color[2],
        ),
        unlit: true,
        ..default()
    });
    let floor_material = materials.add(StandardMaterial {
        base_color: Color::srgb_u8(
            settings.ground_color[0],
            settings.ground_color[1],
            settings.ground_color[2],
        ),
        metallic: 0.2,
        perceptual_roughness: 0.44,
        reflectance: 0.35,
        ..default()
    });

    let structure = &state.structure;
    let total_vector = structure.cell.vectors[0] * supercell[0] as f32
        + structure.cell.vectors[1] * supercell[1] as f32
        + structure.cell.vectors[2] * supercell[2] as f32;
    let center = crystal_to_world(total_vector * 0.5);
    let mut render_atoms = Vec::with_capacity(requested_atoms as usize);
    let mut append_render_atom = |atom: &AtomSite, fractional: Vec3| {
        let position = structure.cell.fractional_to_cartesian(fractional);
        render_atoms.push(RenderAtom {
            position: crystal_to_world(position) - center,
            element: atom.element.clone(),
            label: atom.label.clone(),
            fractional,
            symmetry_rotation: atom.symmetry_rotation,
            magnetic_moment: atom.magnetic_moment,
            occupancy: atom.occupancy,
        });
    };
    for ix in 0..supercell[0] {
        for iy in 0..supercell[1] {
            for iz in 0..supercell[2] {
                let offset = Vec3::new(ix as f32, iy as f32, iz as f32);
                for atom in &source_atoms {
                    append_render_atom(atom, atom.fractional + offset);
                }
            }
        }
    }
    // Fractional coordinates are canonicalized to [0, 1), so a site at 0 is
    // crystallographically identical to one at 1. Draw images only on the
    // outer high faces: this completes the visible unit cell without drawing
    // duplicate atoms on each internal supercell boundary.
    if settings.show_boundary_atoms {
        for atom in &source_atoms {
            for offset in boundary_image_offsets(atom.fractional, supercell)
                .into_iter()
                .skip(1)
            {
                append_render_atom(atom, atom.fractional + offset);
            }
        }
    }

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
    for atom in &render_atoms {
        let style = resolved_style(&atom.element, &overrides);
        let material = atom_materials
            .entry(atom.element.clone())
            .or_insert_with(|| create_atom_material(&mut materials, style))
            .clone();
        let mut radius =
            style.display_radius * settings.atom_scale * atom.occupancy.max(0.15).cbrt();
        if settings.atom_display_mode == AtomDisplayMode::SpaceFilling {
            radius *= 1.55;
        }
        if selection
            .atoms
            .iter()
            .any(|selected| selected.matches(atom))
        {
            commands.spawn((
                Mesh3d(sphere_mesh.clone()),
                MeshMaterial3d(selection_material.clone()),
                Transform::from_translation(atom.position).with_scale(Vec3::splat(radius * 1.18)),
                CrystalVisual,
            ));
        }
        if settings.show_outlines && settings.atom_display_mode != AtomDisplayMode::Wireframe {
            commands.spawn((
                Mesh3d(sphere_mesh.clone()),
                MeshMaterial3d(outline_material.clone()),
                Transform::from_translation(atom.position).with_scale(Vec3::splat(radius * 1.025)),
                CrystalVisual,
            ));
        }
        let mut entity = commands.spawn((
            Name::new(format!("{} {}", atom.element, atom.label)),
            Mesh3d(sphere_mesh.clone()),
            MeshMaterial3d(
                if settings.atom_display_mode == AtomDisplayMode::Wireframe {
                    wireframe_fill_material.clone()
                } else {
                    material
                },
            ),
            Transform::from_translation(atom.position).with_scale(Vec3::splat(radius)),
            AtomPickable {
                atom: atom.clone(),
                radius,
            },
            CrystalVisual,
        ));
        if settings.atom_display_mode == AtomDisplayMode::Wireframe {
            entity.insert((Wireframe, WireframeColor { color: style.color }));
        }
    }

    let bonds = find_bonds(&render_atoms, &settings);
    if settings.show_bonds && settings.atom_display_mode != AtomDisplayMode::SpaceFilling {
        for &(a, b) in &bonds {
            let first = &render_atoms[a];
            let second = &render_atoms[b];
            let midpoint = (first.position + second.position) * 0.5;
            spawn_cylinder_between(
                &mut commands,
                cylinder_mesh.clone(),
                atom_materials[&first.element].clone(),
                first.position,
                midpoint,
                settings.bond_radius,
            );
            spawn_cylinder_between(
                &mut commands,
                cylinder_mesh.clone(),
                atom_materials[&second.element].clone(),
                midpoint,
                second.position,
                settings.bond_radius,
            );
        }
    }

    if settings.show_polyhedra
        && let Some(center) = selection.atoms.first()
        && let Some(center_index) = render_atoms.iter().position(|atom| center.matches(atom))
    {
        let neighbors: Vec<_> = bonds
            .iter()
            .filter_map(|&(a, b)| {
                if a == center_index {
                    Some(render_atoms[b].position)
                } else if b == center_index {
                    Some(render_atoms[a].position)
                } else {
                    None
                }
            })
            .collect();
        if let Some(mesh) = coordination_polyhedron_mesh(&neighbors) {
            let material = materials.add(StandardMaterial {
                base_color: Color::srgba(
                    settings.polyhedron_color[0] as f32 / 255.0,
                    settings.polyhedron_color[1] as f32 / 255.0,
                    settings.polyhedron_color[2] as f32 / 255.0,
                    settings.polyhedron_opacity,
                ),
                alpha_mode: AlphaMode::Blend,
                metallic: 0.08,
                perceptual_roughness: 0.28,
                cull_mode: None,
                ..default()
            });
            commands.spawn((
                Name::new(format!("Coordination polyhedron for {}", center.label)),
                Mesh3d(meshes.add(mesh)),
                MeshMaterial3d(material),
                CrystalVisual,
            ));
        }
    }

    if settings
        .bond_rules
        .iter()
        .any(|rule| rule.enabled && rule.show_polyhedra)
    {
        let mut adjacency = vec![Vec::new(); render_atoms.len()];
        for &(first, second) in &bonds {
            adjacency[first].push(second);
            adjacency[second].push(first);
        }
        let polyhedron_material = materials.add(polyhedron_material(&settings));
        for (rule_index, rule) in settings.bond_rules.iter().enumerate() {
            if !rule.enabled || !rule.show_polyhedra {
                continue;
            }
            for (center_index, center_atom) in render_atoms.iter().enumerate() {
                if !atom_matches(&rule.center, center_atom, settings.bond_match_mode) {
                    continue;
                }
                let neighbors: Vec<_> = adjacency[center_index]
                    .iter()
                    .copied()
                    .filter(|&neighbor_index| {
                        rule_matches_directed(
                            center_atom,
                            &render_atoms[neighbor_index],
                            rule,
                            settings.bond_match_mode,
                        )
                    })
                    .map(|neighbor_index| render_atoms[neighbor_index].position)
                    .collect();
                if let Some(mesh) = coordination_polyhedron_mesh(&neighbors) {
                    commands.spawn((
                        Name::new(format!(
                            "Coordination polyhedron {} for {}",
                            rule_index + 1,
                            center_atom.label
                        )),
                        Mesh3d(meshes.add(mesh)),
                        MeshMaterial3d(polyhedron_material.clone()),
                        CrystalVisual,
                    ));
                }
            }
        }
    }

    if settings.show_imported_moments {
        let material = materials.add(StandardMaterial {
            base_color: Color::srgb_u8(
                settings.imported_moment_color[0],
                settings.imported_moment_color[1],
                settings.imported_moment_color[2],
            ),
            emissive: Color::srgb_u8(
                settings.imported_moment_color[0],
                settings.imported_moment_color[1],
                settings.imported_moment_color[2],
            )
            .to_linear()
                * 0.10,
            metallic: 0.12,
            perceptual_roughness: 0.24,
            ..default()
        });
        let style = AtomVector {
            attachment: VectorAttachment::IndividualAtom,
            label: String::new(),
            element: String::new(),
            position: None,
            components: [0.0; 3],
            coordinates: VectorCoordinates::Cartesian,
            color: settings.imported_moment_color,
            scale: settings.imported_moment_scale,
            penetrate_atom: false,
            add_atom_radius: true,
        };
        for atom in render_atoms
            .iter()
            .filter(|atom| atom.magnetic_moment.is_some())
        {
            let radius = resolved_style(&atom.element, &overrides).display_radius
                * settings.atom_scale
                * atom.occupancy.max(0.15).cbrt();
            spawn_atom_vector(
                &mut commands,
                cylinder_mesh.clone(),
                arrow_head_mesh.clone(),
                material.clone(),
                atom.position,
                crystal_to_world(atom.magnetic_moment.expect("filtered for moments")),
                radius,
                &style,
            );
        }
    }

    for vector in &settings.atom_vectors {
        let material = materials.add(StandardMaterial {
            base_color: Color::srgb_u8(vector.color[0], vector.color[1], vector.color[2]),
            emissive: Color::srgb_u8(vector.color[0], vector.color[1], vector.color[2]).to_linear()
                * 0.10,
            metallic: 0.12,
            perceptual_roughness: 0.24,
            ..default()
        });
        for atom in render_atoms
            .iter()
            .filter(|atom| vector_matches_atom(vector, atom))
        {
            let mut source_vector = Vec3::from_array(vector.components);
            if vector.attachment == VectorAttachment::Site
                && vector.coordinates == VectorCoordinates::Lattice
            {
                let parity = atom.symmetry_rotation.determinant().signum();
                source_vector = atom.symmetry_rotation * source_vector * parity;
            }
            let direction = match vector.coordinates {
                VectorCoordinates::Lattice => {
                    crystal_to_world(structure.cell.fractional_to_cartesian(source_vector))
                }
                VectorCoordinates::Cartesian => crystal_to_world(source_vector),
            };
            let radius = resolved_style(&atom.element, &overrides).display_radius
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
                vector,
            );
        }
    }

    let world_vectors = [
        crystal_to_world(structure.cell.vectors[0] * supercell[0] as f32),
        crystal_to_world(structure.cell.vectors[1] * supercell[1] as f32),
        crystal_to_world(structure.cell.vectors[2] * supercell[2] as f32),
    ];
    let primitive_vectors = state.structure.cell.vectors.map(crystal_to_world);
    match settings.unit_cell_display {
        UnitCellDisplay::Hidden => {}
        UnitCellDisplay::Single => {
            for (from, to) in cell_edges(primitive_vectors) {
                spawn_cell_edge(
                    &mut commands,
                    cylinder_mesh.clone(),
                    sphere_mesh.clone(),
                    cell_material.clone(),
                    from - center,
                    to - center,
                    settings.unit_cell_line_style,
                    settings.unit_cell_line_width,
                );
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
                            if !drawn_edges.insert(cell_edge_key(from, to)) {
                                continue;
                            }
                            spawn_cell_edge(
                                &mut commands,
                                cylinder_mesh.clone(),
                                sphere_mesh.clone(),
                                cell_material.clone(),
                                from - center,
                                to - center,
                                settings.unit_cell_line_style,
                                settings.unit_cell_line_width,
                            );
                        }
                    }
                }
            }
        }
    }
    let extent = world_vectors
        .iter()
        .map(|value| value.length())
        .fold(1.0_f32, f32::max);
    if settings.show_floor {
        let lowest = render_atoms
            .iter()
            .map(|atom| atom.position.y - resolved_style(&atom.element, &overrides).display_radius)
            .fold(0.0_f32, f32::min);
        commands.spawn((
            Mesh3d(
                meshes.add(
                    Plane3d::default()
                        .mesh()
                        .size(extent * 4.0 + 18.0, extent * 4.0 + 18.0),
                ),
            ),
            MeshMaterial3d(floor_material),
            Transform::from_xyz(0.0, lowest - 1.15, 0.0),
            CrystalVisual,
        ));
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

/// Number of periodic images of a site needed to close the positive faces of
/// a displayed cell. The returned count includes the original site.
pub(crate) fn boundary_image_count(fractional: Vec3) -> u64 {
    let boundary_axes = [fractional.x, fractional.y, fractional.z]
        .into_iter()
        .filter(|component| component.abs() <= BOUNDARY_EPSILON)
        .count();
    1_u64 << boundary_axes
}

fn boundary_image_offsets(fractional: Vec3, supercell: [u32; 3]) -> Vec<Vec3> {
    let choices = [
        (fractional.x.abs() <= BOUNDARY_EPSILON).then_some(supercell[0] as f32),
        (fractional.y.abs() <= BOUNDARY_EPSILON).then_some(supercell[1] as f32),
        (fractional.z.abs() <= BOUNDARY_EPSILON).then_some(supercell[2] as f32),
    ];
    let mut offsets = vec![Vec3::ZERO];
    for (axis, boundary_offset) in choices.into_iter().enumerate() {
        let Some(boundary_offset) = boundary_offset else {
            continue;
        };
        let existing = offsets.clone();
        for offset in existing {
            let mut image = offset;
            image[axis] = boundary_offset;
            offsets.push(image);
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

fn polyhedron_material(settings: &VisualSettings) -> StandardMaterial {
    StandardMaterial {
        base_color: Color::srgba(
            settings.polyhedron_color[0] as f32 / 255.0,
            settings.polyhedron_color[1] as f32 / 255.0,
            settings.polyhedron_color[2] as f32 / 255.0,
            settings.polyhedron_opacity,
        ),
        alpha_mode: AlphaMode::Blend,
        metallic: 0.08,
        perceptual_roughness: 0.28,
        cull_mode: None,
        ..default()
    }
}

fn vector_matches_atom(vector: &AtomVector, atom: &RenderAtom) -> bool {
    match vector.attachment {
        VectorAttachment::Site => atom.label == vector.label,
        VectorAttachment::IndividualAtom => {
            atom.element == vector.element
                && atom.label == vector.label
                && vector
                    .position
                    .is_some_and(|position| position.distance_squared(atom.position) < 1.0e-8)
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
    style: &AtomVector,
) {
    let vector_length = vector.length() * style.scale;
    if vector_length < 1.0e-4 {
        return;
    }
    let direction = vector.normalize();
    let head_length = (vector_length * 0.26).clamp(0.16, 0.55);
    let visible_length = vector_length
        + if style.add_atom_radius {
            atom_radius
        } else {
            0.0
        };
    let (start, tip) = if style.penetrate_atom {
        (
            origin - direction * visible_length * 0.5,
            origin + direction * visible_length * 0.5,
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
    let shaft_end = tip - direction * head_length.min(visible_length * 0.8);
    spawn_cylinder_between(
        commands,
        shaft_mesh,
        material.clone(),
        start,
        shaft_end,
        0.045,
    );
    let actual_head_length = tip.distance(shaft_end);
    commands.spawn((
        Mesh3d(head_mesh),
        MeshMaterial3d(material),
        Transform::from_translation(shaft_end + direction * (actual_head_length * 0.5))
            .with_rotation(Quat::from_rotation_arc(Vec3::Y, direction))
            .with_scale(Vec3::new(1.0, actual_head_length / 0.42, 1.0)),
        CrystalVisual,
    ));
}

#[derive(Component, Clone)]
pub(crate) struct AtomPickable {
    pub(crate) atom: RenderAtom,
    pub(crate) radius: f32,
}

fn coordination_polyhedron_mesh(neighbors: &[Vec3]) -> Option<Mesh> {
    if neighbors.len() < 4 || neighbors.len() > 24 {
        return None;
    }
    let center = neighbors.iter().copied().sum::<Vec3>() / neighbors.len() as f32;
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut indices = Vec::new();
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
            }
        }
    }
    if indices.is_empty() {
        return None;
    }
    Some(
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_indices(Indices::U32(indices)),
    )
}

fn resolved_style(symbol: &str, overrides: &ElementOverrides) -> ElementStyle {
    let base = elements::style(symbol);
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

pub(crate) fn default_appearance(symbol: &str) -> ElementAppearance {
    let style = elements::style(symbol);
    ElementAppearance {
        color: style.color.to_srgba().to_u8_array_no_alpha(),
        radius: style.display_radius,
        visible: true,
    }
}

fn create_atom_material(
    materials: &mut Assets<StandardMaterial>,
    style: ElementStyle,
) -> Handle<StandardMaterial> {
    materials.add(StandardMaterial {
        base_color: style.color,
        emissive: style.color.to_linear() * 0.035,
        metallic: 0.08,
        perceptual_roughness: 0.21,
        reflectance: 0.56,
        ..default()
    })
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
fn spawn_cell_edge(
    commands: &mut Commands,
    cylinder_mesh: Handle<Mesh>,
    sphere_mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
    start: Vec3,
    end: Vec3,
    style: CellLineStyle,
    line_width: f32,
) {
    let radius = (0.026 * line_width).clamp(0.008, 0.20);
    let direction = end - start;
    let length = direction.length();
    if length < 1.0e-5 {
        return;
    }
    match style {
        CellLineStyle::Solid => {
            spawn_cylinder_between(commands, cylinder_mesh, material, start, end, radius);
        }
        CellLineStyle::Dotted => {
            let count = (length / (radius * 7.0).max(0.14)).ceil().max(2.0) as usize;
            for index in 0..=count {
                let fraction = index as f32 / count as f32;
                commands.spawn((
                    Mesh3d(sphere_mesh.clone()),
                    MeshMaterial3d(material.clone()),
                    Transform::from_translation(start.lerp(end, fraction))
                        .with_scale(Vec3::splat(radius * 1.55)),
                    CrystalVisual,
                ));
            }
        }
        CellLineStyle::Dashed => {
            let count = (length / (radius * 20.0).max(0.38)).ceil().max(1.0) as usize;
            let dash_span = 1.0 / count as f32;
            for index in 0..count {
                let from = index as f32 * dash_span;
                let to = (from + dash_span * 0.62).min(1.0);
                spawn_cylinder_between(
                    commands,
                    cylinder_mesh.clone(),
                    material.clone(),
                    start.lerp(end, from),
                    start.lerp(end, to),
                    radius,
                );
            }
        }
    }
}

pub(crate) fn atoms_are_bonded(
    first: &RenderAtom,
    second: &RenderAtom,
    settings: &VisualSettings,
) -> bool {
    if settings.bond_rules.is_empty() {
        let covalent = elements::style(&first.element).covalent_radius
            + elements::style(&second.element).covalent_radius;
        let cutoff = (covalent * 1.16).min(settings.bond_cutoff);
        let distance = first.position.distance(second.position);
        return distance > 0.35 && distance <= cutoff;
    }
    settings.bond_rules.iter().any(|rule| {
        rule.enabled
            && (rule_matches_directed(first, second, rule, settings.bond_match_mode)
                || rule_matches_directed(second, first, rule, settings.bond_match_mode))
    })
}

pub(crate) fn rule_matches_directed(
    center: &RenderAtom,
    neighbor: &RenderAtom,
    rule: &BondRule,
    match_mode: BondMatchMode,
) -> bool {
    atom_matches(&rule.center, center, match_mode)
        && atom_matches(&rule.neighbor, neighbor, match_mode)
        && center.position.distance(neighbor.position) > 1.0e-4
        && center.position.distance(neighbor.position) >= rule.min_length.max(0.0)
        && center.position.distance(neighbor.position) <= rule.max_length.max(0.0)
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
    if atoms.len() < 2 {
        return Vec::new();
    }
    let maximum_cutoff = settings
        .bond_rules
        .iter()
        .filter(|rule| rule.enabled)
        .map(|rule| rule.max_length)
        .fold(settings.bond_cutoff, f32::max);
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
                            if atoms_are_bonded(atom, other, settings) {
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
    Vec3::new(value.x, value.z, value.y)
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
    use super::{boundary_image_count, boundary_image_offsets};
    use bevy::prelude::Vec3;

    #[test]
    fn closes_each_boundary_face_once() {
        assert_eq!(boundary_image_count(Vec3::new(0.25, 0.5, 0.75)), 1);
        assert_eq!(boundary_image_count(Vec3::new(0.0, 0.5, 0.75)), 2);
        assert_eq!(boundary_image_count(Vec3::new(0.0, 0.0, 0.0)), 8);

        let offsets = boundary_image_offsets(Vec3::ZERO, [2, 3, 4]);
        assert_eq!(offsets.len(), 8);
        assert!(offsets.contains(&Vec3::new(2.0, 0.0, 0.0)));
        assert!(offsets.contains(&Vec3::new(2.0, 3.0, 4.0)));
    }
}
