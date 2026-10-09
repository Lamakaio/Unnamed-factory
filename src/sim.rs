//! City simulation, scripted in Rhai (`assets/scripts`), and its stats screen.
//!
//! - `init.rhai` builds the `data` map; it runs once loaded, and again on R.
//! - `run.rhai` is one sim tick; ticks run at [`SIM_TICKS_PER_SECOND`] while Enter is held.
//! - Tab toggles the stats screen, generated from the structure of `data`.

use std::hash::{BuildHasher, Hash, Hasher};

use bevy::{
    asset::{AssetLoader, AsyncReadExt, LoadContext},
    input::common_conditions::{input_just_pressed, input_pressed},
    platform::collections::HashMap,
    prelude::*,
    scene::SceneBox,
};
use foldhash::fast::FixedState;
use rhai::{Engine, ImmutableString, Scope};

use crate::ui::label;

#[derive(Asset, TypePath, Debug)]
pub struct RhaiScript {
    text: String,
    ast: Option<rhai::AST>,
}

#[derive(Default, TypePath)]
pub struct RhaiScriptLoader;

impl AssetLoader for RhaiScriptLoader {
    type Asset = RhaiScript;

    type Settings = ();

    type Error = anyhow::Error;

    async fn load(
        &self,
        reader: &mut dyn bevy::asset::io::Reader,
        _settings: &Self::Settings,
        _load_context: &mut LoadContext<'_>,
    ) -> Result<Self::Asset, Self::Error> {
        let mut buf = String::new();

        reader.read_to_string(&mut buf).await?;

        Ok(RhaiScript {
            text: buf,
            ast: None,
        })
    }

    fn extensions(&self) -> &[&str] {
        &["rhai"]
    }
}

/// Simulation ticks per second while Enter is held.
const SIM_TICKS_PER_SECOND: f64 = 20.;
const INIT_SCRIPT: &str = "scripts/init.rhai";
const RUN_SCRIPT: &str = "scripts/run.rhai";
const SIM_SCREEN_BACKGROUND: Color = Color::srgba(0., 0., 0., 0.9);

/// Scripted simulation state.
#[derive(Resource)]
pub struct Sim {
    init: Handle<RhaiScript>,
    run: Handle<RhaiScript>,
    initialized: bool,
    /// Holds the `data` map shared by the scripts.
    scope: Scope<'static>,
    engine: Engine,
    /// Numeric values of `data`, keyed by the hash of their path.
    values: HashMap<u64, f64>,
}

impl FromWorld for Sim {
    fn from_world(world: &mut World) -> Self {
        let asset_server = world.resource::<AssetServer>();
        let mut scope = Scope::new();
        scope.push("data", rhai::Map::new());
        Self {
            init: asset_server.load(INIT_SCRIPT),
            run: asset_server.load(RUN_SCRIPT),
            initialized: false,
            scope,
            engine: Engine::new(),
            values: default(),
        }
    }
}

impl Sim {
    fn data(&self) -> Option<&rhai::Map> {
        self.scope.get_value_ref("data")
    }

    /// Refresh [`Sim::values`] from the `data` map.
    fn collect_values(&mut self) {
        let Sim { scope, values, .. } = self;
        if let Some(data) = scope.get_value_ref::<rhai::Map>("data") {
            collect_values(values, data, &mut Vec::new());
        }
    }
}

pub struct SimPlugin;

impl Plugin for SimPlugin {
    fn build(&self, app: &mut App) {
        app.init_asset::<RhaiScript>()
            .init_asset_loader::<RhaiScriptLoader>()
            .init_resource::<Sim>()
            .insert_resource(Time::<Fixed>::from_hz(SIM_TICKS_PER_SECOND))
            .add_systems(
                Update,
                (
                    init_sim.run_if(init_requested),
                    toggle_sim_screen.run_if(input_just_pressed(KeyCode::Tab)),
                    update_stats.run_if(resource_changed::<Sim>),
                ),
            )
            .add_systems(FixedUpdate, tick_sim.run_if(input_pressed(KeyCode::Enter)))
            .add_observer(spawn_sim_screen);
    }
}

/// The sim data was (re)initialized.
#[derive(Event)]
struct SimInitialized;

/// Run condition: the init script is loaded and the sim is not initialized yet, or R is pressed.
fn init_requested(
    sim: Res<Sim>,
    scripts: Res<Assets<RhaiScript>>,
    keyboard: Res<ButtonInput<KeyCode>>,
) -> bool {
    scripts.contains(&sim.init) && (!sim.initialized || keyboard.just_pressed(KeyCode::KeyR))
}

/// Reset the sim data and run the init script.
fn init_sim(mut commands: Commands, mut sim: ResMut<Sim>, scripts: Res<Assets<RhaiScript>>) {
    info!("Init script");
    let Some(script) = scripts.get(&sim.init) else {
        return;
    };
    let Sim { engine, scope, .. } = &mut *sim;
    scope.set_value("data", rhai::Map::new());
    if let Err(error) = engine.run_with_scope(scope, &script.text) {
        error!("{INIT_SCRIPT} failed: {error}");
    }
    sim.initialized = true;
    sim.values.clear();
    sim.collect_values();
    commands.trigger(SimInitialized);
}

/// Run one sim tick. Script errors are logged, so scripts can be fixed and hot-reloaded.
fn tick_sim(mut sim: ResMut<Sim>, mut scripts: ResMut<Assets<RhaiScript>>) {
    if !sim.initialized {
        return;
    }
    let Some(mut script) = scripts.get_mut(&sim.run) else {
        return;
    };
    if script.ast.is_none() {
        match sim.engine.compile_with_scope(&sim.scope, &script.text) {
            Ok(ast) => script.ast = Some(ast),
            Err(error) => {
                error!("{RUN_SCRIPT} does not compile: {error}");
                return;
            }
        }
    }
    let Some(ast) = &script.ast else {
        return;
    };
    let Sim { engine, scope, .. } = &mut *sim;
    if let Err(error) = engine.run_ast_with_scope(scope, ast) {
        error!("{RUN_SCRIPT} failed: {error}");
    }
    sim.collect_values();
}

fn collect_values(
    values: &mut HashMap<u64, f64>,
    data: &rhai::Map,
    path: &mut Vec<ImmutableString>,
) {
    for (name, value) in data {
        path.push(name.into());
        if let Some(map) = value.clone().try_cast::<rhai::Map>() {
            collect_values(values, &map, path);
        } else if let Some(value) = value.clone().try_cast::<f64>() {
            values.insert(path_hash(path), value);
        }
        path.pop();
    }
}

fn path_hash(path: &[ImmutableString]) -> u64 {
    let mut hasher = FixedState::default().build_hasher();
    path.hash(&mut hasher);
    hasher.finish()
}

/// Root of the stats screen.
#[derive(Component, Default, Clone)]
struct SimScreen;

/// A numeric value of the sim data, displayed on the stats screen.
#[derive(Component, Default, Clone)]
struct Stat {
    /// Hash of the value's path in `data`.
    id: u64,
    name: String,
}

/// (Re)build the stats screen from the structure of the sim data.
fn spawn_sim_screen(
    _initialized: On<SimInitialized>,
    mut commands: Commands,
    sim: Res<Sim>,
    screens: Query<Entity, With<SimScreen>>,
) {
    for screen in &screens {
        commands.entity(screen).despawn();
    }
    let stats = sim
        .data()
        .map(|data| stat_scenes(data, &mut Vec::new()))
        .unwrap_or_default();
    commands.queue_spawn_scene(sim_screen(stats));
}

fn stat_scenes(data: &rhai::Map, path: &mut Vec<ImmutableString>) -> Vec<Box<dyn SceneBox>> {
    let mut scenes: Vec<Box<dyn SceneBox>> = Vec::new();
    for (name, value) in data {
        path.push(name.into());
        if let Some(map) = value.clone().try_cast::<rhai::Map>() {
            let children = stat_scenes(&map, path);
            scenes.push(Box::new(stat_group(name.to_string(), children)));
        } else if value.is::<f64>() {
            scenes.push(Box::new(stat_value(name.to_string(), path_hash(path))));
        }
        path.pop();
    }
    scenes
}

fn sim_screen(stats: Vec<Box<dyn SceneBox>>) -> impl Scene {
    bsn! {
        SimScreen
        Node {
            justify_content: JustifyContent::SpaceEvenly,
            flex_direction: FlexDirection::Row,
            flex_wrap: FlexWrap::Wrap,
            margin: percent(5),
        }
        BackgroundColor(SIM_SCREEN_BACKGROUND)
        Visibility::Hidden
        Children [ {stats} ]
    }
}

fn stat_group(name: String, children: Vec<Box<dyn SceneBox>>) -> impl Scene {
    let border = BorderColor::all(Color::hsv(rand::random_range(0.0..360.0), 0.3, 0.8));
    bsn! {
        Node {
            flex_direction: FlexDirection::Column,
            border: px(5),
            margin: px(10),
            flex_wrap: FlexWrap::Wrap,
        }
        border
        Children [
            @label(name)
            Node { margin: px(10) }
            --
            {children}
        ]
    }
}

fn stat_value(name: String, id: u64) -> impl Scene {
    let text = format!("{name} : -");
    bsn! {
        @label(text)
        Node { margin: px(3) }
        Stat { id, name }
    }
}

fn toggle_sim_screen(mut screens: Query<&mut Visibility, With<SimScreen>>) {
    for mut visibility in &mut screens {
        visibility.toggle_visible_hidden();
    }
}

fn update_stats(sim: Res<Sim>, mut stats: Query<(&mut Text, &Stat)>) {
    for (mut text, stat) in &mut stats {
        let value = sim.values.get(&stat.id).copied().unwrap_or(f64::NAN);
        text.0 = format!("{} : {value:.2}", stat.name);
    }
}
