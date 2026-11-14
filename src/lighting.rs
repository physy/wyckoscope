use bevy::{camera::Exposure, core_pipeline::tonemapping::Tonemapping, prelude::*};

use crate::state::*;

#[allow(clippy::type_complexity)]
pub(crate) fn sync_environment(
    settings: Res<VisualSettings>,
    mut background_camera: Single<&mut Camera, With<BackgroundCamera>>,
    mut camera: Single<
        (&Transform, &OrbitCamera, &mut Exposure, &mut Tonemapping),
        (With<OrbitCamera>, Without<ExportCamera>, Without<LightRole>),
    >,
    mut ambient: ResMut<AmbientLight>,
    mut lights: Query<
        (
            &LightRole,
            &mut Transform,
            &mut Visibility,
            Option<&mut PointLight>,
            Option<&mut SpotLight>,
            Option<&mut DirectionalLight>,
        ),
        Without<OrbitCamera>,
    >,
) {
    background_camera.clear_color = ClearColorConfig::Custom(Color::srgb_u8(
        settings.background[0],
        settings.background[1],
        settings.background[2],
    ));
    let (camera_transform, orbit, exposure, tonemapping) = &mut *camera;
    exposure.ev100 = settings.exposure_ev;
    **tonemapping = if settings.hdr_tonemapping {
        Tonemapping::TonyMcMapface
    } else {
        Tonemapping::None
    };

    let lighting = &settings.lighting;
    ambient.color = Color::srgb_u8(
        lighting.ambient_color[0],
        lighting.ambient_color[1],
        lighting.ambient_color[2],
    );
    ambient.brightness = if lighting.ambient_enabled {
        lighting.ambient_brightness
    } else {
        0.0
    };

    let key_position = match lighting.key_mode {
        LightMode::Camera => {
            let camera_up = camera_transform.rotation * Vec3::Y;
            let camera_right = camera_transform.rotation * Vec3::X;
            let camera_back = camera_transform.rotation * Vec3::Z;
            let azimuth = Quat::from_axis_angle(
                camera_up.normalize_or_zero(),
                lighting.key_follow_azimuth.to_radians(),
            );
            let elevation = Quat::from_axis_angle(
                camera_right.normalize_or_zero(),
                lighting.key_follow_elevation.to_radians(),
            );
            let direction = (elevation * azimuth * camera_back).normalize_or_zero();
            // Keep the source on a fixed-radius shell around the orbit target. Point and spot
            // lights attenuate with distance, so coupling this distance to camera zoom causes an
            // unwanted exposure shift while simply magnifying the structure.
            orbit.target + direction * lighting.key_follow_distance
        }
        LightMode::World => Vec3::from_array(lighting.key_position),
    };
    let key_target = orbit.target + Vec3::from_array(lighting.key_target_offset);

    for (role, mut transform, mut visibility, point, spot, directional) in &mut lights {
        match role {
            LightRole::KeyPoint => {
                *visibility = if lighting.key_enabled && lighting.key_kind == KeyLightKind::Point {
                    Visibility::Visible
                } else {
                    Visibility::Hidden
                };
                transform.translation = key_position;
                if let Some(mut light) = point {
                    light.color = color_from_rgb(lighting.key_color);
                    light.intensity = lighting.key_intensity;
                    light.range = lighting.key_range;
                    light.radius = lighting.key_radius;
                    light.shadows_enabled = lighting.key_shadows;
                    light.shadow_depth_bias = lighting.key_shadow_depth_bias;
                    light.shadow_normal_bias = lighting.key_shadow_normal_bias;
                    light.shadow_map_near_z = lighting.key_shadow_near_z;
                }
            }
            LightRole::KeySpot => {
                *visibility = if lighting.key_enabled && lighting.key_kind == KeyLightKind::Spot {
                    Visibility::Visible
                } else {
                    Visibility::Hidden
                };
                *transform =
                    Transform::from_translation(key_position).looking_at(key_target, Vec3::Y);
                if let Some(mut light) = spot {
                    light.color = color_from_rgb(lighting.key_color);
                    light.intensity = lighting.key_intensity;
                    light.range = lighting.key_range;
                    light.radius = lighting.key_radius;
                    light.shadows_enabled = lighting.key_shadows;
                    light.shadow_depth_bias = lighting.key_shadow_depth_bias;
                    light.shadow_normal_bias = lighting.key_shadow_normal_bias;
                    light.shadow_map_near_z = lighting.key_shadow_near_z;
                    light.outer_angle = lighting.spot_outer_angle.to_radians();
                    light.inner_angle = lighting
                        .spot_inner_angle
                        .min(lighting.spot_outer_angle)
                        .to_radians();
                }
            }
            LightRole::Fill => {
                *visibility = if lighting.fill_enabled {
                    Visibility::Visible
                } else {
                    Visibility::Hidden
                };
                transform.rotation = rotation_from_degrees(lighting.fill_rotation);
                if let Some(mut light) = directional {
                    light.color = color_from_rgb(lighting.fill_color);
                    light.illuminance = lighting.fill_illuminance;
                    light.shadows_enabled = lighting.fill_shadows;
                    light.shadow_depth_bias = lighting.fill_shadow_depth_bias;
                    light.shadow_normal_bias = lighting.fill_shadow_normal_bias;
                }
            }
            LightRole::AxisKey => {
                *visibility = if lighting.axis_key_enabled {
                    Visibility::Visible
                } else {
                    Visibility::Hidden
                };
                transform.rotation = rotation_from_degrees(lighting.axis_key_rotation);
                if let Some(mut light) = directional {
                    light.color = color_from_rgb(lighting.axis_key_color);
                    light.illuminance = lighting.axis_key_illuminance;
                }
            }
            LightRole::AxisFill => {
                *visibility = if lighting.axis_fill_enabled {
                    Visibility::Visible
                } else {
                    Visibility::Hidden
                };
                transform.rotation = rotation_from_degrees(lighting.axis_fill_rotation);
                if let Some(mut light) = directional {
                    light.color = color_from_rgb(lighting.axis_fill_color);
                    light.illuminance = lighting.axis_fill_illuminance;
                }
            }
        }
    }
}

fn color_from_rgb(color: [u8; 3]) -> Color {
    Color::srgb_u8(color[0], color[1], color[2])
}

fn rotation_from_degrees(rotation: [f32; 3]) -> Quat {
    Quat::from_euler(
        EulerRot::XYZ,
        rotation[0].to_radians(),
        rotation[1].to_radians(),
        rotation[2].to_radians(),
    )
}
