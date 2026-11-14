mod camera;
mod export;
mod file_io;
mod lighting;
mod scene;
mod state;
mod ui;

use bevy::{
    pbr::wireframe::WireframePlugin,
    prelude::*,
    window::{PresentMode, WindowResolution},
};
use bevy_egui::{EguiGlobalSettings, EguiPlugin, EguiPrimaryContextPass};
use wyckoscope::parser;

use camera::{orbit_camera, select_atom, sync_orientation_axes, update_camera_viewports};
use export::process_export;
use file_io::{keyboard_shortcuts, load_dropped_file, poll_async_dialogs};
use lighting::sync_environment;
use scene::{rebuild_scene, setup};
use state::{
    AppState, AsyncDialogs, ElementOverrides, ExportState, InspectorLayout, SceneDirty,
    SceneStatus, SelectionPointer, SelectionState, VisualSettings,
};
use ui::interface;

const SAMPLE_CIF: &str = include_str!("../assets/samples/perovskite.cif");
pub(crate) const AXIS_VIEW_SIZE: f32 = 180.0;
pub(crate) const AXIS_VIEW_MARGIN: f32 = 18.0;
pub(crate) const AXIS_RENDER_LAYER: usize = 1;
pub(crate) const MAX_RENDER_ATOMS: u64 = 150_000;

fn main() {
    let structure = parser::parse_text(SAMPLE_CIF, "cif").expect("bundled sample CIF is valid");

    App::new()
        .insert_resource(ClearColor(Color::WHITE))
        .insert_resource(AmbientLight {
            color: Color::srgb(0.82, 0.87, 0.98),
            brightness: 520.0,
            affects_lightmapped_meshes: true,
        })
        .insert_resource(AppState {
            structure,
            file_name: "Built-in · SrTiO₃".to_string(),
            error: None,
        })
        .init_resource::<VisualSettings>()
        .init_resource::<ElementOverrides>()
        .init_resource::<AsyncDialogs>()
        .init_resource::<ExportState>()
        .init_resource::<SceneStatus>()
        .init_resource::<InspectorLayout>()
        .init_resource::<SelectionState>()
        .init_resource::<SelectionPointer>()
        .insert_resource(SceneDirty {
            rebuild: true,
            reset_camera: true,
        })
        .insert_resource(EguiGlobalSettings {
            auto_create_primary_context: false,
            ..default()
        })
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Wyckoscope — Crystal Structure Viewer".to_string(),
                resolution: WindowResolution::new(1440, 900),
                present_mode: PresentMode::AutoVsync,
                resizable: true,
                ..default()
            }),
            ..default()
        }))
        .add_plugins(EguiPlugin::default())
        .add_plugins(WireframePlugin::default())
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (
                load_dropped_file,
                poll_async_dialogs,
                keyboard_shortcuts,
                select_atom,
                rebuild_scene,
                orbit_camera,
                update_camera_viewports,
                sync_orientation_axes,
                sync_environment,
                process_export,
            )
                .chain(),
        )
        .add_systems(EguiPrimaryContextPass, interface)
        .run();
}
