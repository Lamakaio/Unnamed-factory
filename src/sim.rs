use std::hash::{BuildHasher, Hash, Hasher};

use bevy::asset::{AssetLoader, AsyncReadExt, LoadContext};
use bevy::ecs::relationship::RelatedSpawnerCommands;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use foldhash::fast::{FixedState, FoldHasher, RandomState};
use rhai::Scope;
use rhai::{Engine, ImmutableString};

#[derive(Asset, TypePath, Debug)]
pub struct RhaiScript {
    text: String,
    ast: Option<rhai::AST>,
}

#[derive(Default)]
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

#[derive(Resource)]
pub struct Sim {
    init: Handle<RhaiScript>,
    run: Handle<RhaiScript>,
    initialized: bool,
    scope: rhai::Scope<'static>, //dynamic storing a boxed sim_data
    engine: Engine,
    values: HashMap<u64, f64>,
}

impl Default for Sim {
    fn default() -> Self {
        let engine = Engine::new();
        let mut scope = Scope::new();
        scope.push("data", rhai::Map::new());
        Self {
            init: Default::default(),
            run: Default::default(),
            scope,
            initialized: false,
            engine,
            values: default(),
        }
    }
}

pub struct SimPlugin;
impl Plugin for SimPlugin {
    fn build(&self, app: &mut App) {
        app.init_asset::<RhaiScript>();
        app.init_asset_loader::<RhaiScriptLoader>();
        app.insert_resource(Sim::default());
        app.add_systems(Startup, (init_rhai,));
        app.add_systems(
            Update,
            (
                run_rhai,
                make_sim_ui.after(run_rhai),
                get_values.after(run_rhai),
                update_ui.after(make_sim_ui).after(get_values),
            ),
        );
    }
}

fn init_rhai(mut sim: ResMut<Sim>, asset_server: Res<AssetServer>) {
    sim.init = asset_server.load("scripts/init.rhai");
    sim.run = asset_server.load("scripts/run.rhai");
}

fn run_rhai(
    mut sim: ResMut<Sim>,
    input: Res<ButtonInput<KeyCode>>,
    mut scripts: ResMut<Assets<RhaiScript>>,
) -> Result {
    //todo better error handling
    //Initialize simulation
    if !sim.initialized || input.just_pressed(KeyCode::KeyR) {
        info!("Init script");
        //reset sim data
        *sim.scope.get_mut("data").ok_or("critical failure")? = rhai::Map::new().into();
        if let Some(sc) = scripts.get_mut(&sim.init) {
            let Sim { engine, scope, .. } = &mut *sim;
            engine.run_with_scope(scope, &*sc.text)?;
        }
        sim.initialized = true;
    }
    if let Some(sc) = scripts.get_mut(&sim.run) {
        if sc.ast.is_none() {
            sc.ast = sim.engine.compile_with_scope(&sim.scope, &sc.text).ok();
        }

        if let Some(ast) = &sc.ast {
            let Sim { engine, scope, .. } = &mut *sim;

            engine.run_ast_with_scope(scope, ast)?;
        }
    }

    Ok(())
}

#[derive(Component)]
struct Stat(u64, ImmutableString);

fn spawn_on(
    parent: &mut RelatedSpawnerCommands<ChildOf>,
    data: &rhai::Map,
    font: &Handle<Font>,
    path: &mut Vec<rhai::ImmutableString>,
) {
    for (name, v) in data.iter() {
        path.push(name.into());
        if let Some(map) = v.clone().try_cast::<rhai::Map>() {
            parent
                .spawn((
                    Node {
                        flex_direction: FlexDirection::Column,
                        border: UiRect::all(Val::Px(5.0)),
                        margin: UiRect::all(Val::Px(10.)),
                        ..default()
                    },
                    BorderColor(Color::hsv(rand::random_range(0.0..360.0), 1.0, 1.0)),
                ))
                .with_children(|parent| {
                    parent.spawn((
                        Node {
                            margin: UiRect::all(Val::Px(10.)),
                            ..default()
                        },
                        Text(name.to_string()),
                        TextFont {
                            font: font.clone(),
                            ..default()
                        },
                        Label,
                    ));
                    spawn_on(parent, &map, font, path);
                });
        } else if let Some(f) = v.clone().try_cast::<f64>() {
            let mut h = FixedState::default().build_hasher();
            path.hash(&mut h);
            parent.spawn((
                Node {
                    margin: UiRect::all(Val::Px(10.)),
                    ..default()
                },
                Text(format!("{} : {}", name, f)),
                TextFont {
                    font: font.clone(),
                    ..default()
                },
                Label,
                Stat(h.finish(), name.clone().into()),
            ));
        }
        path.pop();
    }
}
#[derive(Component)]
struct MainNode;

fn make_sim_ui(
    mut commands: Commands,
    sim: Res<Sim>,
    asset_server: Res<AssetServer>,
    main_node_query: Option<Single<Entity, With<MainNode>>>,
    input: Res<ButtonInput<KeyCode>>,
) {
    if sim.initialized && (main_node_query.is_none() || input.just_pressed(KeyCode::KeyR)) {
        if let Some(e) = main_node_query {
            commands.entity(*e).despawn();
        }
        let font = asset_server.load("fonts/FiraSans-Bold.ttf");
        let data: &rhai::Map = sim.scope.get_value_ref("data").unwrap();
        commands
            .spawn((
                Node {
                    display: Display::Flex,
                    justify_content: JustifyContent::SpaceEvenly,
                    flex_direction: FlexDirection::Row,
                    flex_wrap: FlexWrap::Wrap,
                    ..default()
                },
                MainNode,
            ))
            .with_children(|parent| {
                let mut path = vec![];
                spawn_on(parent, data, &font, &mut path);
            });
    }
}

fn get_values_rec(
    values: &mut HashMap<u64, f64>,
    data: &rhai::Map,
    path: &mut Vec<rhai::ImmutableString>,
) {
    for (name, v) in data.iter() {
        path.push(name.into());
        if let Some(map) = v.clone().try_cast::<rhai::Map>() {
            get_values_rec(values, &map, path);
        } else if let Some(f) = v.clone().try_cast::<f64>() {
            let mut h = FixedState::default().build_hasher();
            path.hash(&mut h);
            values.insert(h.finish(), f);
        }
        path.pop();
    }
}

fn get_values(mut sim: ResMut<Sim>) {
    let Sim { scope, values, .. } = &mut *sim;
    let data: &rhai::Map = scope.get_value_ref("data").unwrap();
    let mut path = Vec::new();
    get_values_rec(values, data, &mut path);
}

fn update_ui(sim: Res<Sim>, mut stat_query: Query<(&mut Text, &Stat)>) {
    for (mut text, Stat(id, name)) in &mut stat_query {
        text.0 = format!(
            "{} : {:.2}",
            name,
            sim.values.get(id).copied().unwrap_or(f64::NAN)
        );
    }
}
