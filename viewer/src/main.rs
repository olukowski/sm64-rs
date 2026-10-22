//! ROM-backed static terrain viewer; not gameplay or a full RDP renderer.
mod capture;
mod mesh;
mod skybox;
mod visual;

use anyhow::{Context, Result, bail};
use bevy::{
    app::{AppExit, ScheduleRunnerPlugin},
    camera::{Exposure, RenderTarget},
    core_pipeline::tonemapping::{DebandDither, Tonemapping},
    input::mouse::AccumulatedMouseMotion,
    light::Skybox,
    pbr::{DistanceFog, FogFalloff},
    prelude::{
        AlphaMode, App, Assets, ButtonInput, Camera3d, ClearColor, Color, Commands, Component,
        DefaultPlugins, EulerRot, FontSize, Handle, Image, KeyCode, Mesh, Mesh3d, MeshMaterial3d,
        MessageWriter, MouseButton, Msaa, Node, On, PerspectiveProjection, PluginGroup,
        PositionType, Projection, Quat, Query, Res, ResMut, Resource, StandardMaterial, Startup,
        Text, TextFont, Time, Transform, Update, Vec3, Visibility, Window, WindowPlugin, With,
        Without, default, error, info, px,
    },
    render::{
        render_resource::{TextureFormat, TextureUsages},
        view::screenshot::{Screenshot, ScreenshotCaptured},
    },
    ui::IsDefaultUiCamera,
    window::ExitCondition,
    winit::WinitPlugin,
};
use clap::{Parser, ValueEnum};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Parser)]
#[command(
    about = "ROM-backed textured static Bob-omb Battlefield viewer (approximate materials, no gameplay)"
)]
struct Args {
    #[arg(long, env = "SM64_ROM")]
    rom: PathBuf,
    #[arg(long, default_value_t = 1)]
    area: u8,
    #[arg(long, value_enum, default_value_t = Mode::Textured)]
    mode: Mode,
    /// Initially show cyan collision edges over original render geometry.
    #[arg(long)]
    collision_overlay: bool,
    /// Add a static ROM center-idle Mario snapshot on verified flat ground (not gameplay).
    #[arg(long)]
    mario: bool,
    /// Static presentation yaw in degrees; used by --mario/--showcase, not gameplay.
    #[arg(long, default_value_t = 0.0, value_parser = parse_angle, allow_hyphen_values = true)]
    mario_yaw: f32,
    /// Render the ROM ocean-sky panorama with approximate host projection and distance fog.
    #[arg(long)]
    skybox: bool,
    /// Composed static Mario/BOB scene with sky, fixed home camera and clean UI (not gameplay).
    #[arg(long)]
    showcase: bool,
    /// Reproducible camera position, comma-separated original SM64 units.
    #[arg(long, value_parser = parse_position, requires = "look_at", allow_hyphen_values = true)]
    camera: Option<Vec3>,
    #[arg(long, value_parser = parse_position, requires = "camera", allow_hyphen_values = true)]
    look_at: Option<Vec3>,
    /// Render a fixed-resolution PNG without a window, then exit. Never overwrites.
    #[arg(long)]
    shot: Option<PathBuf>,
    #[arg(long, default_value_t = 1920, value_parser = clap::value_parser!(u32).range(320..=8192))]
    width: u32,
    #[arg(long, default_value_t = 1080, value_parser = clap::value_parser!(u32).range(240..=8192))]
    height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Mode {
    Textured,
    Collision,
}

#[derive(Component)]
enum RenderKind {
    Textured,
    Collision,
    Overlay,
}
#[derive(Component)]
struct StatusLabel;

#[derive(Resource)]
struct Scene {
    geometry: Vec<visual::VisualBatch>,
    textures: Vec<sm64_gfx::Texture>,
    collision: Option<Mesh>,
    edges: Option<Mesh>,
    sky: Option<Image>,
    shadow: Option<(Mesh, Image)>,
    home: Transform,
    center: Vec3,
    radius: f32,
    speed: f32,
    label: String,
    mode: Mode,
    overlay: bool,
    clean: bool,
}

impl Scene {
    fn status(&self) -> String {
        format!(
            "{}\nMode: {:?} | collision overlay: {}\nRMB look | WASD/QE fly | Shift fast | R reset | V mode | C overlay | H labels | Esc exit",
            self.label,
            self.mode,
            if self.overlay { "on" } else { "off" }
        )
    }
    fn visibility(&self, kind: &RenderKind) -> Visibility {
        let visible = match kind {
            RenderKind::Textured => self.mode == Mode::Textured,
            RenderKind::Collision => self.mode == Mode::Collision,
            RenderKind::Overlay => self.mode == Mode::Textured && self.overlay,
        };
        if visible {
            Visibility::Visible
        } else {
            Visibility::Hidden
        }
    }
}

fn parse_position(value: &str) -> std::result::Result<Vec3, String> {
    let numbers: Vec<_> = value
        .split(',')
        .map(str::parse::<f32>)
        .collect::<std::result::Result<_, _>>()
        .map_err(|_| "expected x,y,z numbers".to_string())?;
    if numbers.len() != 3
        || !numbers
            .iter()
            .all(|n| n.is_finite() && n.abs() <= 1_000_000.)
    {
        return Err("expected three finite coordinates within +/-1,000,000 SM64 units".into());
    }
    Ok(Vec3::new(numbers[0], numbers[1], numbers[2]))
}

#[derive(Resource)]
struct Shot {
    path: Option<PathBuf>,
    frames: u32,
    requested: bool,
    target: Option<Handle<Image>>,
    width: u32,
    height: u32,
}

fn parse_angle(value: &str) -> std::result::Result<f32, String> {
    let value = value
        .parse::<f32>()
        .map_err(|_| "expected finite degrees".to_string())?;
    if !value.is_finite() || value.abs() > 360_000.0 {
        return Err("expected finite degrees within +/-360000".into());
    }
    Ok(value)
}

fn validate_render_options(args: &Args) -> Result<()> {
    if u64::from(args.width) * u64::from(args.height) > 16_777_216 {
        bail!("render dimensions exceed the 16-megapixel budget");
    }
    if args.showcase && (args.mode != Mode::Textured || args.collision_overlay) {
        bail!("--showcase requires textured mode without a collision overlay");
    }
    Ok(())
}

fn fixed_camera(eye: Vec3, target: Vec3) -> Result<Transform> {
    let delta = target - eye;
    if !eye.is_finite() || !target.is_finite() || delta.length_squared() < 0.01 {
        bail!("camera and look-at must be distinct finite positions");
    }
    let up = if delta.normalize().dot(Vec3::Y).abs() > 0.999 {
        Vec3::Z
    } else {
        Vec3::Y
    };
    Ok(Transform::from_translation(eye * mesh::SCALE).looking_at(target * mesh::SCALE, up))
}

fn far_for_bounds(eye: Vec3, center: Vec3, radius: f32) -> f32 {
    ((eye.distance(center) + radius) * 1.2 + 10.).max(1000.)
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
    validate_render_options(&args)?;
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
    let mut visuals = visual::load(&level, area)?;
    let mut shadow = None;
    if args.mario || args.showcase {
        let floor = mesh::showcase_floor(collision)?;
        let mario = visual::load_mario(&rom, floor, args.mario_yaw)?;
        eprintln!(
            "Mario bounds (viewer units): {:?} .. {:?}",
            mario.min, mario.max
        );
        visuals.append(mario)?;
        shadow = Some(visual::contact_shadow(floor));
    }
    let sky = if args.skybox || args.showcase {
        let background = level.segment_data.resolve_range(0x0e0004a8, 8)?;
        if background[..4] != [0x19, 0, 0, 0] {
            bail!("unsupported BOB background declaration; no skybox guessed");
        }
        Some(skybox::cube_image(&sm64_gfx::skybox::decode_panorama(
            &level.segment_data,
            0x0a020000,
        )?)?)
    } else {
        None
    };
    for diagnostic in &visuals.diagnostics {
        eprintln!("Visual: {diagnostic}");
    }
    let center = (visuals.min + visuals.max) * 0.5;
    let radius = ((visuals.max - visuals.min).length() * 0.5).max(1.0);
    // A bounding sphere fits vertically AND horizontally, even in portrait windows.
    let aspect = args.width as f32 / args.height as f32;
    let half_fov = (std::f32::consts::FRAC_PI_6.tan() * aspect.min(1.0)).atan();
    let distance = radius / half_fov.sin() * 1.15;
    let home = if let (Some(eye), Some(target)) = (args.camera, args.look_at) {
        fixed_camera(eye, target)?
    } else if args.showcase {
        fixed_camera(Vec3::new(-4800., 1224., 500.), Vec3::new(0., 1500., -3000.))?
    } else {
        Transform::from_translation(center + Vec3::new(1.0, 0.85, 1.2).normalize() * distance)
            .looking_at(center, Vec3::Y)
    };
    let label = format!(
        "{} | area {} | STATIC PRESENTATION / APPROXIMATE MATERIALS\n{} ROM triangles ({} textured), {} textures, {} geo draws / {} commands\n{} collision triangles ({} degenerate skipped) | 100 SM64 units = 1 viewer unit",
        level.name,
        area.index,
        visuals.triangles,
        visuals.textured_triangles,
        visuals.textures.len(),
        visuals.draws,
        visuals.commands,
        converted.positions.len() / 3,
        converted.skipped
    );
    eprintln!("{label}");
    let scene = Scene {
        geometry: visuals.batches,
        textures: visuals.textures,
        collision: Some(converted.into_mesh()),
        edges: Some(visual::collision_lines(collision)?),
        sky,
        shadow,
        home,
        center,
        radius,
        speed: radius * 0.35,
        label,
        mode: args.mode,
        overlay: args.collision_overlay,
        clean: args.showcase,
    };
    let offscreen = args.shot.is_some();
    let mut app = App::new();
    app.insert_resource(scene)
        .insert_resource(Shot {
            path: args.shot,
            frames: 0,
            requested: false,
            target: None,
            width: args.width,
            height: args.height,
        })
        .insert_resource(ClearColor(Color::srgb(0.37, 0.62, 0.85)));
    if offscreen {
        app.add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: None,
                    exit_condition: ExitCondition::DontExit,
                    ..default()
                })
                .disable::<WinitPlugin>(),
        )
        .add_plugins(ScheduleRunnerPlugin::run_loop(Duration::from_secs_f64(
            1.0 / 60.0,
        )));
    } else {
        app.add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "sm64-rs | static showcase, not gameplay | H labels".into(),
                resolution: (args.width, args.height).into(),
                ..default()
            }),
            ..default()
        }));
    }
    let exit = app
        .add_systems(Startup, setup)
        .add_systems(Update, (fly, toggle_view, request_capture))
        .run();
    if exit.is_error() {
        bail!("viewer exited with an error (see screenshot/render diagnostics)");
    }
    Ok(())
}

fn overlay_material() -> StandardMaterial {
    // WGPU forbids depth bias on LineList topology. Edges are offset slightly
    // along their collision normals instead; do not apply triangle-only state.
    StandardMaterial {
        base_color: Color::srgba(0.05, 1.0, 1.0, 0.85),
        unlit: true,
        alpha_mode: AlphaMode::Blend,
        cull_mode: None,
        ..default()
    }
}

fn setup(
    mut commands: Commands,
    mut scene: ResMut<Scene>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut shot: ResMut<Shot>,
) {
    use bevy::render::render_resource::Face;
    let texture_handles: Vec<_> = scene
        .textures
        .drain(..)
        .map(|texture| images.add(visual::image(texture)))
        .collect();
    let terrain_visibility = scene.visibility(&RenderKind::Textured);
    for batch in scene.geometry.drain(..) {
        commands.spawn((
            Mesh3d(meshes.add(batch.mesh)),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::WHITE,
                base_color_texture: batch.texture.map(|index| texture_handles[index].clone()),
                alpha_mode: batch.alpha,
                unlit: true,
                cull_mode: batch.cull_back.then_some(Face::Back),
                double_sided: !batch.cull_back,
                depth_bias: if matches!(batch.layer, 2 | 6) {
                    1.0
                } else {
                    0.0
                },
                ..default()
            })),
            batch.placement,
            RenderKind::Textured,
            terrain_visibility,
        ));
    }
    commands.spawn((
        Mesh3d(meshes.add(scene.collision.take().expect("startup once"))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::WHITE,
            unlit: true,
            cull_mode: None,
            double_sided: true,
            ..default()
        })),
        RenderKind::Collision,
        scene.visibility(&RenderKind::Collision),
    ));
    commands.spawn((
        Mesh3d(meshes.add(scene.edges.take().expect("startup once"))),
        MeshMaterial3d(materials.add(overlay_material())),
        RenderKind::Overlay,
        scene.visibility(&RenderKind::Overlay),
    ));
    if let Some((mesh, image)) = scene.shadow.take() {
        commands.spawn((
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color_texture: Some(images.add(image)),
                unlit: true,
                alpha_mode: AlphaMode::Blend,
                cull_mode: None,
                ..default()
            })),
            RenderKind::Textured,
            terrain_visibility,
        ));
    }
    let sky_handle = scene.sky.take().map(|image| images.add(image));
    let mut camera = commands.spawn((
        Camera3d::default(),
        IsDefaultUiCamera,
        Msaa::Sample4,
        Tonemapping::None,
        DebandDither::Disabled,
        Projection::Perspective(PerspectiveProjection {
            fov: std::f32::consts::FRAC_PI_3,
            near: 0.05,
            far: far_for_bounds(scene.home.translation, scene.center, scene.radius),
            ..default()
        }),
        scene.home,
    ));
    if shot.path.is_some() {
        let mut target =
            Image::new_target_texture(shot.width, shot.height, TextureFormat::Rgba8UnormSrgb, None);
        target.texture_descriptor.usage |= TextureUsages::COPY_SRC;
        let target = images.add(target);
        camera.insert(RenderTarget::Image(target.clone().into()));
        shot.target = Some(target);
    }
    if let Some(image) = sky_handle {
        camera.insert((
            Skybox {
                image: Some(image),
                brightness: 1.0 / Exposure::default().exposure(),
                ..default()
            },
            DistanceFog {
                color: Color::srgb(0.70, 0.82, 0.95),
                falloff: FogFalloff::Linear {
                    start: 80.0,
                    end: 200.0,
                },
                ..default()
            },
        ));
    }
    commands.spawn((
        Text::new(scene.status()),
        StatusLabel,
        if scene.clean {
            Visibility::Hidden
        } else {
            Visibility::Visible
        },
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

fn toggle_view(
    keys: Res<ButtonInput<KeyCode>>,
    mut scene: ResMut<Scene>,
    mut entities: Query<(&RenderKind, &mut Visibility), Without<StatusLabel>>,
    mut labels: Query<(&mut Text, &mut Visibility), With<StatusLabel>>,
) {
    if !keys.just_pressed(KeyCode::KeyV)
        && !keys.just_pressed(KeyCode::KeyC)
        && !keys.just_pressed(KeyCode::KeyH)
    {
        return;
    }
    if keys.just_pressed(KeyCode::KeyV) {
        scene.mode = if scene.mode == Mode::Textured {
            Mode::Collision
        } else {
            Mode::Textured
        };
    }
    if keys.just_pressed(KeyCode::KeyC) {
        scene.overlay = !scene.overlay;
    }
    if keys.just_pressed(KeyCode::KeyH) {
        scene.clean = !scene.clean;
    }
    for (kind, mut visibility) in &mut entities {
        *visibility = scene.visibility(kind);
    }
    for (mut label, mut visibility) in &mut labels {
        label.0 = scene.status();
        *visibility = if scene.clean {
            Visibility::Hidden
        } else {
            Visibility::Visible
        };
    }
}

fn fly(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    scene: Res<Scene>,
    mut cameras: Query<(&mut Transform, &mut Projection), With<Camera3d>>,
    mut exit: MessageWriter<AppExit>,
) {
    if keys.just_pressed(KeyCode::Escape) {
        exit.write(AppExit::Success);
    }
    for (mut camera, mut projection) in &mut cameras {
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
        if let Projection::Perspective(perspective) = &mut *projection {
            perspective.far = far_for_bounds(camera.translation, scene.center, scene.radius);
        }
    }
}

fn request_capture(
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
    let (width, height) = (shot.width, shot.height);
    let Some(target) = shot.target.clone() else {
        error!("offscreen screenshot target was not initialized");
        exit.write(AppExit::error());
        return;
    };
    commands.spawn(Screenshot::image(target)).observe(
        move |event: On<ScreenshotCaptured>, mut exit: MessageWriter<AppExit>| {
            let result = (|| -> Result<()> {
                let image = event
                    .image
                    .clone()
                    .try_into_dynamic()
                    .with_context(|| "converting screenshot")?;
                if image.width() != width || image.height() != height {
                    bail!("offscreen readback dimensions differ from requested {width}x{height}");
                }
                capture::write_png(&path, &image)?;
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
    fn fixed_camera_coordinates_reject_nonfinite_or_unbounded_input() {
        assert_eq!(
            parse_position("-100,200,300").unwrap(),
            Vec3::new(-100., 200., 300.)
        );
        let eye = parse_position("1000000,1000000,1000000").unwrap() * mesh::SCALE;
        assert!(far_for_bounds(eye, Vec3::ZERO, 100.) > eye.length() + 100.);
        for value in [
            "1,2",
            "1,2,3,4",
            "NaN,0,0",
            "inf,0,0",
            "1000001,0,0",
            "bad,0,0",
        ] {
            assert!(parse_position(value).is_err(), "{value}");
        }
    }

    #[test]
    fn showcase_options_and_degenerate_camera_inputs_are_checked_without_rom_or_gpu() {
        let args = Args::try_parse_from([
            "viewer",
            "--rom",
            "authored.z64",
            "--showcase",
            "--width",
            "1001",
            "--height",
            "777",
        ])
        .unwrap();
        assert!(validate_render_options(&args).is_ok());
        let args = Args::try_parse_from([
            "viewer",
            "--rom",
            "authored.z64",
            "--showcase",
            "--mode",
            "collision",
        ])
        .unwrap();
        assert!(validate_render_options(&args).is_err());
        let args = Args::try_parse_from([
            "viewer",
            "--rom",
            "authored.z64",
            "--width",
            "8192",
            "--height",
            "8192",
        ])
        .unwrap();
        assert!(validate_render_options(&args).is_err());
        assert!(
            Args::try_parse_from(["viewer", "--rom", "authored.z64", "--camera", "1,2,3"]).is_err()
        );
        assert!(fixed_camera(Vec3::ZERO, Vec3::ZERO).is_err());
        assert!(fixed_camera(Vec3::NAN, Vec3::ZERO).is_err());
        let vertical = fixed_camera(Vec3::Y * 100., Vec3::ZERO).unwrap();
        assert!(vertical.rotation.is_finite());
        for invalid in ["NaN", "inf", "-inf", "360001", "bad"] {
            assert!(parse_angle(invalid).is_err());
        }
        assert_eq!(parse_angle("-45").unwrap(), -45.);
    }

    #[test]
    fn line_overlay_material_has_no_triangle_only_depth_bias() {
        assert_eq!(overlay_material().depth_bias, 0.);
    }

    #[test]
    fn modes_keep_collision_overlay_separate_from_original_render_meshes() {
        let mut scene = Scene {
            geometry: vec![],
            textures: vec![],
            collision: None,
            edges: None,
            sky: None,
            shadow: None,
            home: Transform::IDENTITY,
            center: Vec3::ZERO,
            radius: 1.,
            speed: 1.,
            label: "fixture".into(),
            mode: Mode::Textured,
            overlay: false,
            clean: false,
        };
        assert_eq!(scene.visibility(&RenderKind::Textured), Visibility::Visible);
        assert_eq!(scene.visibility(&RenderKind::Collision), Visibility::Hidden);
        scene.overlay = true;
        assert_eq!(scene.visibility(&RenderKind::Overlay), Visibility::Visible);
        scene.mode = Mode::Collision;
        assert_eq!(scene.visibility(&RenderKind::Textured), Visibility::Hidden);
        assert_eq!(
            scene.visibility(&RenderKind::Collision),
            Visibility::Visible
        );
        assert_eq!(scene.visibility(&RenderKind::Overlay), Visibility::Hidden);
    }

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
