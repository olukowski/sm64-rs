//! First ROM-backed viewer: static collision only, no gameplay or textured graphics.
mod mesh;

use anyhow::{Context, Result, bail};
use bevy::{
    app::AppExit,
    core_pipeline::tonemapping::{DebandDither, Tonemapping},
    input::mouse::AccumulatedMouseMotion,
    prelude::{
        App, Assets, ButtonInput, Camera3d, ClearColor, Color, Commands, DefaultPlugins, EulerRot,
        FontSize, KeyCode, Mesh, Mesh3d, MeshMaterial3d, MessageWriter, MouseButton, Node, On,
        PerspectiveProjection, PluginGroup, PositionType, Projection, Quat, Query, Res, ResMut,
        Resource, StandardMaterial, Startup, Text, TextFont, Time, Transform, Update, Vec3, Window,
        WindowPlugin, With, default, error, info, px,
    },
    render::view::screenshot::{Screenshot, ScreenshotCaptured},
};
use clap::Parser;
use std::{
    fs::OpenOptions,
    io::BufWriter,
    path::{Path, PathBuf},
};

#[derive(Parser)]
#[command(
    about = "ROM-backed Bob-omb Battlefield static collision viewer (not textured graphics/gameplay)"
)]
struct Args {
    #[arg(long, env = "SM64_ROM")]
    rom: PathBuf,
    #[arg(long, default_value_t = 1)]
    area: u8,
    /// Save a new PNG and exit after writing it. Prefer an ignored .cache/ path.
    #[arg(long)]
    shot: Option<PathBuf>,
    #[arg(long, default_value_t = 1280, value_parser = clap::value_parser!(u32).range(320..=8192))]
    width: u32,
    #[arg(long, default_value_t = 800, value_parser = clap::value_parser!(u32).range(240..=8192))]
    height: u32,
}

#[derive(Resource)]
struct Scene {
    geometry: Option<Mesh>,
    home: Transform,
    speed: f32,
    label: String,
}

#[derive(Resource)]
struct Shot {
    path: Option<PathBuf>,
    frames: u32,
    requested: bool,
}

fn validate_shot(path: &Path, rom: &Path) -> Result<()> {
    if path.extension().and_then(|s| s.to_str()) != Some("png") {
        bail!("--shot must have a .png extension");
    }
    if path.symlink_metadata().is_ok() {
        bail!(
            "refusing existing screenshot destination {}",
            path.display()
        );
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let destination = parent
        .canonicalize()
        .with_context(|| "screenshot parent must already exist")?
        .join(
            path.file_name()
                .with_context(|| "screenshot needs a filename")?,
        );
    if destination == rom.canonicalize()? {
        bail!("screenshot destination aliases the ROM");
    }
    Ok(())
}

fn main() -> Result<()> {
    let args = Args::parse();
    if let Some(path) = &args.shot {
        validate_shot(path, &args.rom)?;
    }
    let rom = sm64_rom::Rom::open(&args.rom)?;
    let level = sm64_level::load_bob(&rom)?;
    for diagnostic in &level.diagnostics {
        eprintln!("{diagnostic}");
    }
    let area = level
        .areas
        .iter()
        .find(|a| a.index == args.area)
        .with_context(|| "requested area does not exist")?;
    let collision = area
        .collision
        .as_ref()
        .with_context(|| "requested area has no static collision")?;
    let converted = mesh::convert(collision)?;
    let center = (converted.min + converted.max) * 0.5;
    let radius = ((converted.max - converted.min).length() * 0.5).max(1.0);
    // A bounding sphere fits vertically AND horizontally, even in portrait windows.
    let aspect = args.width as f32 / args.height as f32;
    let half_fov = (std::f32::consts::FRAC_PI_6.tan() * aspect.min(1.0)).atan();
    let distance = radius / half_fov.sin() * 1.15;
    let home =
        Transform::from_translation(center + Vec3::new(1.0, 0.85, 1.2).normalize() * distance)
            .looking_at(center, Vec3::Y);
    let label = format!(
        "{} | area {} | STATIC COLLISION ONLY\n{} triangles ({} degenerate skipped) | diagnostic colors\nScale: 1 viewer unit = 100 SM64 units (Y up)\nRMB look | WASD fly | Q/E down/up | Shift fast | R reset | Esc exit",
        level.name,
        area.index,
        converted.positions.len() / 3,
        converted.skipped
    );
    eprintln!("{label}");
    let exit = App::new()
        .insert_resource(Scene { geometry: Some(converted.into_mesh()), home, speed: radius * 0.35, label })
        .insert_resource(Shot { path: args.shot, frames: 0, requested: false })
        .insert_resource(ClearColor(Color::srgb(0.035, 0.05, 0.075)))
        .add_plugins(DefaultPlugins.set(WindowPlugin { primary_window: Some(Window { title: "SM64 — STATIC COLLISION viewer | RMB look | WASD/QE fly | Shift | R reset | Esc".into(), resolution: (args.width, args.height).into(), ..default() }), ..default() }))
        .add_systems(Startup, setup)
        .add_systems(Update, (fly, capture))
        .run();
    if exit.is_error() {
        bail!("viewer exited with an error (see screenshot/render diagnostics)");
    }
    Ok(())
}

fn setup(
    mut commands: Commands,
    mut scene: ResMut<Scene>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.spawn((
        Mesh3d(meshes.add(scene.geometry.take().expect("startup once"))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::WHITE,
            unlit: true,
            cull_mode: None,
            double_sided: true,
            ..default()
        })),
    ));
    commands.spawn((
        Camera3d::default(),
        Tonemapping::None,
        DebandDither::Disabled,
        Projection::Perspective(PerspectiveProjection {
            fov: std::f32::consts::FRAC_PI_3,
            near: 0.05,
            far: 10000.,
            ..default()
        }),
        scene.home,
    ));
    commands.spawn((
        Text::new(scene.label.clone()),
        TextFont {
            font_size: FontSize::Px(15.),
            ..default()
        },
        Node {
            position_type: PositionType::Absolute,
            top: px(12),
            left: px(12),
            right: px(12),
            ..default()
        },
    ));
}

fn fly(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    scene: Res<Scene>,
    mut cameras: Query<&mut Transform, With<Camera3d>>,
    mut exit: MessageWriter<AppExit>,
) {
    if keys.just_pressed(KeyCode::Escape) {
        exit.write(AppExit::Success);
    }
    for mut camera in &mut cameras {
        if keys.just_pressed(KeyCode::KeyR) {
            *camera = scene.home;
        }
        if buttons.pressed(MouseButton::Right) {
            let (yaw, pitch, _) = camera.rotation.to_euler(EulerRot::YXZ);
            camera.rotation = Quat::from_euler(
                EulerRot::YXZ,
                yaw - motion.delta.x * 0.003,
                (pitch - motion.delta.y * 0.003).clamp(-1.54, 1.54),
                0.,
            );
        }
        let mut direction = Vec3::ZERO;
        for (key, axis) in [
            (KeyCode::KeyW, *camera.forward()),
            (KeyCode::KeyS, *camera.back()),
            (KeyCode::KeyA, *camera.left()),
            (KeyCode::KeyD, *camera.right()),
            (KeyCode::KeyQ, Vec3::NEG_Y),
            (KeyCode::KeyE, Vec3::Y),
        ] {
            if keys.pressed(key) {
                direction += axis;
            }
        }
        let boost = if keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight) {
            4.
        } else {
            1.
        };
        camera.translation +=
            direction.normalize_or_zero() * scene.speed * boost * time.delta_secs().min(0.1);
    }
}

fn capture(
    mut commands: Commands,
    mut shot: ResMut<Shot>,
    time: Res<Time>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(path) = shot.path.clone() else {
        return;
    };
    shot.frames += 1;
    if time.elapsed_secs() > 60. {
        error!("screenshot timed out before completion: {}", path.display());
        exit.write(AppExit::error());
        return;
    }
    // Let pipelines and UI settle before requesting readback, not merely a fixed exit timer.
    if shot.requested || shot.frames < 40 || time.elapsed_secs() < 3.0 {
        return;
    }
    shot.requested = true;
    commands.spawn(Screenshot::primary_window()).observe(
        move |event: On<ScreenshotCaptured>, mut exit: MessageWriter<AppExit>| {
            let result = (|| -> Result<()> {
                let image = event
                    .image
                    .clone()
                    .try_into_dynamic()
                    .with_context(|| "converting screenshot")?;
                // create_new is atomic: never truncate a ROM, symlink, or preexisting output.
                let file = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&path)
                    .with_context(|| "creating new screenshot")?;
                let mut writer = BufWriter::new(file);
                image
                    .write_to(&mut writer, image::ImageFormat::Png)
                    .with_context(|| "encoding screenshot")?;
                use std::io::Write;
                writer.flush().with_context(|| "flushing screenshot")?;
                writer
                    .get_ref()
                    .sync_all()
                    .with_context(|| "syncing screenshot")?;
                Ok(())
            })();
            match result {
                Ok(()) => {
                    info!("Screenshot saved: {}", path.display());
                    exit.write(AppExit::Success);
                }
                Err(error) => {
                    error!("Screenshot failed: {error:#}");
                    exit.write(AppExit::error());
                }
            }
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screenshot_paths_never_accept_existing_files_or_rom() {
        let directory =
            std::env::temp_dir().join(format!("sm64-viewer-path-test-{}", std::process::id()));
        std::fs::create_dir(&directory).unwrap();
        let rom = directory.join("source.png");
        std::fs::write(&rom, b"authored placeholder, not a ROM").unwrap();
        assert!(validate_shot(&directory.join("new.png"), &rom).is_ok());
        assert!(validate_shot(&rom, &rom).is_err());
        let output = directory.join("existing.png");
        std::fs::write(&output, b"keep").unwrap();
        assert!(validate_shot(&output, &rom).is_err());
        assert!(validate_shot(&directory.join("new.jpg"), &rom).is_err());
        assert!(validate_shot(&directory.join("missing/new.png"), &rom).is_err());
        assert_eq!(std::fs::read(&output).unwrap(), b"keep");
        std::fs::remove_dir_all(directory).unwrap();
    }
}
