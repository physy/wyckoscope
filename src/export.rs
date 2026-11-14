use bevy::{
    camera::{CameraOutputMode, Exposure, RenderTarget},
    core_pipeline::tonemapping::Tonemapping,
    post_process::bloom::Bloom,
    prelude::*,
    render::{
        render_resource::{BlendState, TextureFormat, TextureUsages},
        view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk},
    },
};

use crate::state::*;

pub(crate) fn process_export(
    mut commands: Commands,
    mut export: ResMut<ExportState>,
    settings: Res<VisualSettings>,
    main_camera: Single<(&Transform, &Projection, &Tonemapping, &Exposure), With<OrbitCamera>>,
    mut images: ResMut<Assets<Image>>,
) {
    if export.stage.is_none()
        && !export.capturing
        && let Some(request) = export.request.take()
    {
        let width = request.width.clamp(256, 8192);
        let height = request.height.clamp(256, 8192);
        let mut image = Image::new_target_texture(width, height, TextureFormat::bevy_default());
        image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
        let image_handle = images.add(image);
        let (transform, projection, tonemapping, exposure) = main_camera.into_inner();
        let background_camera = commands
            .spawn((
                Camera2d,
                Camera {
                    order: -2,
                    target: RenderTarget::Image(image_handle.clone().into()),
                    clear_color: ClearColorConfig::Custom(Color::srgb_u8(
                        settings.background[0],
                        settings.background[1],
                        settings.background[2],
                    )),
                    ..default()
                },
                ExportCamera,
            ))
            .id();
        let camera = commands
            .spawn((
                Camera3d::default(),
                Camera {
                    order: -1,
                    target: RenderTarget::Image(image_handle.clone().into()),
                    clear_color: ClearColorConfig::Custom(Color::NONE),
                    output_mode: CameraOutputMode::Write {
                        blend_state: Some(BlendState::ALPHA_BLENDING),
                        clear_color: ClearColorConfig::None,
                    },
                    ..default()
                },
                projection.clone(),
                *tonemapping,
                *exposure,
                Bloom::NATURAL,
                Msaa::Sample4,
                *transform,
                ExportCamera,
            ))
            .id();
        export.stage = Some(ExportStage {
            image: image_handle,
            camera,
            background_camera,
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
        let background_camera = stage.background_camera;
        commands
            .spawn(Screenshot::image(stage.image))
            .observe(save_to_disk(stage.path))
            .observe(
                move |_: On<ScreenshotCaptured>,
                      mut commands: Commands,
                      mut images: ResMut<Assets<Image>>,
                      mut export: ResMut<ExportState>| {
                    commands.entity(camera).despawn();
                    commands.entity(background_camera).despawn();
                    images.remove(&image);
                    export.capturing = false;
                    export.message = Some(format!(
                        "Saved {}",
                        path.file_name().and_then(|v| v.to_str()).unwrap_or("PNG")
                    ));
                },
            );
    }
}
