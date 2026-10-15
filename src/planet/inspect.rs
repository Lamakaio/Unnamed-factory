//! Inspecting the planet: clicking the ground logs what generation knows about it (position,
//! heights, water, climate, wind, the nearest rivers and springs) and the camera view that
//! shows it (`UF_PLANET_CAMERA`).
//!
//! Reporting issues: the "Report issue" button (or F6), then a click on the problem, then a
//! description (Enter saves, Esc cancels) writes `issues/issue-<n>.md` (the description, the
//! report and the view) and `issues/issue-<n>.png` (a screenshot with the spot marked), for
//! the developers (and agents) to read and fix.
//!
//! `UF_PLANET_REPORT=lon,lat[;lon,lat...]` (degrees) logs the report of those points once the
//! planet is shown (what a click there would log).

use std::{fmt::Write as _, path::PathBuf};

use bevy::{
    input::{
        ButtonState,
        keyboard::{Key, KeyboardInput},
    },
    prelude::*,
    render::view::screenshot::{Screenshot, save_to_disk},
};
use worldgen::RiverEnd;

use super::{PlanetChunk, PlanetWorld, camera::PlanetCamera};
use crate::{AppState, ui::label};

/// Where the issue reports go.
const ISSUES_DIR: &str = "issues";

pub(super) struct InspectPlugin;

impl Plugin for InspectPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Inspector>()
            .add_observer(on_ground_click)
            .add_systems(OnEnter(AppState::InGame), spawn_ui)
            .add_systems(Update, report_requested.run_if(in_state(AppState::InGame)))
            .add_systems(
                Update,
                (
                    start_report.run_if(bevy::input::common_conditions::input_just_pressed(KeyCode::F6)),
                    type_description,
                    show_state,
                )
                    .chain()
                    .run_if(in_state(AppState::InGame)),
            );
    }
}

/// What the inspector is doing.
#[derive(Resource, Default)]
pub enum Inspector {
    /// Clicks log the ground.
    #[default]
    Idle,
    /// Waiting for the click on the problem.
    Picking,
    /// Typing the description of the problem at `point`.
    Writing { point: Vec3, report: String, text: String },
    /// Saving: the screenshot is taken next frame (the marker drawn).
    Saving { point: Vec3, file: PathBuf, frames: u8 },
}

impl Inspector {
    /// Whether keys go to the description (the camera ignores them).
    pub fn typing(&self) -> bool {
        matches!(self, Inspector::Writing { .. })
    }
}

#[derive(Component)]
struct StatusText;

fn spawn_ui(mut commands: Commands) {
    commands
        .queue_spawn_scene(label("Report issue (F6)"))
        .insert((
            Node {
                position_type: PositionType::Absolute,
                top: px(10),
                right: px(10),
                padding: UiRect::all(px(6)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.1, 0.1, 0.12, 0.8)),
        ))
        .observe(|mut click: On<PointerClick>, mut inspector: ResMut<Inspector>| {
            click.propagate(false);
            *inspector = Inspector::Picking;
        });
    commands.queue_spawn_scene(label("")).insert((
        Node {
            position_type: PositionType::Absolute,
            bottom: px(14),
            left: px(14),
            right: px(14),
            ..default()
        },
        StatusText,
    ));
}

/// The reports asked for by `UF_PLANET_REPORT`, once (the camera placed).
fn report_requested(world: Res<PlanetWorld>, camera: Query<&PlanetCamera>, mut done: Local<bool>) {
    let (false, Ok(camera), Ok(points)) = (*done, camera.single(), std::env::var("UF_PLANET_REPORT")) else {
        return;
    };
    *done = true;
    for point in points.split(';') {
        let values: Vec<f32> = point.split(',').filter_map(|v| v.trim().parse().ok()).collect();
        let [longitude, latitude] = values[..] else {
            continue;
        };
        let (longitude, latitude) = (longitude.to_radians(), latitude.to_radians());
        let direction = Vec3::new(latitude.cos() * longitude.cos(), latitude.sin(), latitude.cos() * longitude.sin());
        let position = direction * (world.surface.radius + world.surface.height_at(direction));
        info!("{}", report(&world, position, camera));
    }
}

fn start_report(mut inspector: ResMut<Inspector>) {
    if matches!(*inspector, Inspector::Idle) {
        *inspector = Inspector::Picking;
    }
}

/// Log the ground clicked, or pick it for a report.
fn on_ground_click(
    click: On<PointerClick>,
    chunks: Query<(), With<PlanetChunk>>,
    world: Option<Res<PlanetWorld>>,
    camera: Query<&PlanetCamera>,
    mut inspector: ResMut<Inspector>,
) {
    let (true, Some(world), Some(position), Ok(camera)) = (
        chunks.contains(click.entity),
        world,
        click.hit.position,
        camera.single(),
    ) else {
        return;
    };
    if click.button != PointerButton::Primary {
        return;
    }
    let report = report(&world, position, camera);
    info!("{report}");
    if matches!(*inspector, Inspector::Picking) {
        *inspector = Inspector::Writing { point: position, report, text: String::new() };
    }
}

/// Keys typed into the description; Enter saves, Escape cancels.
fn type_description(
    mut commands: Commands,
    mut keys: MessageReader<KeyboardInput>,
    mut inspector: ResMut<Inspector>,
    mut gizmos: Gizmos,
) {
    let typed: Vec<KeyboardInput> = keys.read().filter(|k| k.state == ButtonState::Pressed).cloned().collect();
    // The marker on the spot, while writing and in the screenshot.
    if let Inspector::Writing { point, .. } | Inspector::Saving { point, .. } = &*inspector {
        let up = point.normalize();
        gizmos.line(*point, *point + up * 3., Color::srgb(1., 0.1, 0.6));
        gizmos.circle(Isometry3d::new(*point + up * 0.05, Quat::from_rotation_arc(Vec3::Z, up)), 0.6, Color::srgb(1., 0.1, 0.6));
    }
    match &mut *inspector {
        Inspector::Picking => {
            if typed.iter().any(|k| k.logical_key == Key::Escape) {
                *inspector = Inspector::Idle;
            }
        }
        Inspector::Writing { point, report, text } => {
            for key in &typed {
                match &key.logical_key {
                    Key::Escape => {
                        *inspector = Inspector::Idle;
                        return;
                    }
                    Key::Enter => {
                        let (point, file) = (*point, save_report(report, text));
                        *inspector = match file {
                            Some(file) => Inspector::Saving { point, file, frames: 2 },
                            None => Inspector::Idle,
                        };
                        return;
                    }
                    Key::Backspace => {
                        text.pop();
                    }
                    _ => {
                        if let Some(characters) = &key.text {
                            text.extend(characters.chars().filter(|c| !c.is_control()));
                        }
                    }
                }
            }
        }
        Inspector::Saving { file, frames, .. } => {
            // (A frame for the status text to clear and the marker to show.)
            if *frames > 0 {
                *frames -= 1;
                return;
            }
            commands.spawn(Screenshot::primary_window()).observe(save_to_disk(file.with_extension("png")));
            info!("Issue saved to {}", file.display());
            *inspector = Inspector::Idle;
        }
        Inspector::Idle => {}
    }
}

/// Write the issue (the next free number); returns its path (without extension).
fn save_report(report: &str, text: &str) -> Option<PathBuf> {
    let dir = PathBuf::from(ISSUES_DIR);
    if let Err(error) = std::fs::create_dir_all(&dir) {
        error!("Could not create {}: {error}", dir.display());
        return None;
    }
    let number = (1..).find(|n| !dir.join(format!("issue-{n}.md")).exists())?;
    let file = dir.join(format!("issue-{number}"));
    let contents = format!(
        "# Issue {number}\n\n{}\n\nScreenshot: issue-{number}.png (the spot marked in pink)\n\n```\n{report}\n```\n",
        text.trim()
    );
    match std::fs::write(file.with_extension("md"), contents) {
        Ok(()) => Some(file),
        Err(error) => {
            error!("Could not write the issue: {error}");
            None
        }
    }
}

fn show_state(inspector: Res<Inspector>, mut status: Query<&mut Text, With<StatusText>>) {
    if !inspector.is_changed() {
        return;
    }
    let message = match &*inspector {
        Inspector::Idle | Inspector::Saving { .. } => String::new(),
        Inspector::Picking => "Click the problem on the ground (Esc: cancel)".into(),
        Inspector::Writing { text, .. } => format!("Describe the problem, Enter saves, Esc cancels: {text}_"),
    };
    for mut status in &mut status {
        status.0 = message.clone();
    }
}

/// What generation knows about a point of the ground.
fn report(world: &PlanetWorld, position: Vec3, camera: &PlanetCamera) -> String {
    let surface = &world.surface;
    let direction = position.normalize();
    let (face, p) = surface.face_coords(direction);
    let (lon, lat) = (direction.z.atan2(direction.x).to_degrees(), direction.y.asin().to_degrees());
    let height = surface.height(face, p);
    let mut out = String::new();
    let _ = writeln!(out, "Ground at lon {lon:.2} lat {lat:.2} (face {face}, cell {:.1} {:.1})", p.x, p.y);
    let _ = writeln!(
        out,
        "  height {height:.3} (cells {:.3}), lake depth {:.3}, ocean {:.2}, wetness {:.2}, marsh/salt {:.2}",
        surface.heights.bilinear(face, p),
        surface.lake_depth.bilinear(face, p),
        surface.ocean.bilinear(face, p),
        surface.wetness(face, p),
        surface.marsh.bilinear(face, p),
    );
    let (t, rain) = world.climate.months_at(direction, height);
    let koppen = world.climate.koppen_at(direction, height);
    let _ = writeln!(
        out,
        "  climate {koppen:?}: {:.1} °C mean ({:.1} to {:.1}), {:.0} mm/year",
        t.iter().sum::<f32>() / 12.,
        t.iter().copied().fold(f32::MAX, f32::min),
        t.iter().copied().fold(f32::MIN, f32::max),
        rain.iter().sum::<f32>()
    );
    if let Some(weights) = &surface.weights {
        let w = weights.each_ref().map(|f| f.bilinear(face, p));
        let _ = writeln!(
            out,
            "  ground weights: dryness {:.2} forest {:.2} desert {:.2} snow {:.2} cold {:.2}",
            w[0], w[1], w[2], w[3], w[4]
        );
    }
    let (wind, strength) = surface.wind_at(face, p);
    let (east, north) = super::camera::east_north(direction);
    let _ = writeln!(
        out,
        "  wind toward {:.0}° (from north), prevailing {:.1} m/s, strength {:.1} m/s",
        wind.dot(east).atan2(wind.dot(north)).to_degrees().rem_euclid(360.),
        wind.length(),
        strength
    );
    // The nearest rivers (one point each).
    let mut nearest: Vec<(f32, usize, usize)> = surface
        .rivers
        .iter()
        .enumerate()
        .filter_map(|(r, river)| {
            river
                .points
                .iter()
                .enumerate()
                .map(|(k, point)| (point.direction.distance(direction) * surface.radius, r, k))
                .min_by(|a, b| a.0.total_cmp(&b.0))
        })
        .collect();
    nearest.sort_by(|a, b| a.0.total_cmp(&b.0));
    for &(distance, r, k) in nearest.iter().take(3).filter(|n| n.0 < 30.) {
        let river = &surface.rivers[r];
        let point = river.points[k];
        let end = match river.end {
            RiverEnd::Sea => "the sea".to_string(),
            RiverEnd::Lake(_) => "a lake".to_string(),
            RiverEnd::Junction(parent) => format!("river {parent}"),
        };
        let _ = writeln!(
            out,
            "  river {r}: {distance:.2} from point {k} of {}, width {:.2}, level {:.3}, depth {:.2}, ends in {end}",
            river.points.len(),
            point.width,
            point.level,
            point.depth
        );
    }
    for spring in surface.springs() {
        let distance = spring.direction.distance(direction) * surface.radius;
        if distance < 10. {
            let _ = writeln!(out, "  spring of river {} ({:?}), {distance:.2} away", spring.river, match spring.kind {
                worldgen::planet_springs::PlanetSpringKind::Pond { .. } => "pond",
                worldgen::planet_springs::PlanetSpringKind::Hillside => "hillside",
            });
        }
    }
    let _ = write!(out, "  view: UF_PLANET_CAMERA={}", super::camera::describe(camera));
    out
}
