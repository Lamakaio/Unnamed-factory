//! In-game UI: the build menu listing every building, built with BSN scenes.

use bevy::{
    asset::LoadedFolder,
    color::palettes::basic::RED,
    ecs::system::SystemParam,
    input::mouse::{MouseScrollUnit, MouseWheel},
    picking::{
        hover::{HoverMap, Hovered},
        pointer::PointerId,
    },
    prelude::*,
    text::FontSourceTemplate,
    ui::Pressed,
    ui_widgets::{Activate, Button},
};

use crate::build::{Building, Buildings, StartPlacement};

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                spawn_build_menu.run_if(buildings_loaded),
                update_scroll_position,
                update_button_appearance,
            ),
        );
    }
}

/// Font used by all UI text.
pub const UI_FONT: &str = "fonts/FiraSans-Bold.ttf";
const FONT_SIZE: f32 = 20.;
const LINE_HEIGHT: f32 = 21.;
const BUTTON_HEIGHT: f32 = 2. * LINE_HEIGHT;

const NORMAL_BUTTON: Color = Color::srgb(0.15, 0.15, 0.15);
const HOVERED_BUTTON: Color = Color::srgb(0.25, 0.25, 0.25);
const PRESSED_BUTTON: Color = Color::srgb(0.35, 0.75, 0.35);
const PANEL_BACKGROUND: Color = Color::srgb(0.10, 0.10, 0.10);

/// A build menu button, starting the placement of its building.
#[derive(Component, FromTemplate)]
struct PartButton(Handle<Building>);

/// Run condition: every building definition in the `buildings` folder is loaded.
fn buildings_loaded(folder: Res<Buildings>, asset_server: Res<AssetServer>) -> bool {
    asset_server.is_loaded_with_dependencies(&folder.0)
}

/// Spawn the build menu once all buildings are loaded.
fn spawn_build_menu(
    mut commands: Commands,
    folder: Res<Buildings>,
    folders: Res<Assets<LoadedFolder>>,
    buildings: Res<Assets<Building>>,
    mut spawned: Local<bool>,
) -> Result {
    if *spawned {
        return Ok(());
    }
    *spawned = true;
    let folder = folders
        .get(&folder.0)
        .ok_or("buildings folder is not loaded")?;
    let mut entries: Vec<_> = folder
        .handles
        .iter()
        .filter_map(|handle| handle.clone().try_typed::<Building>().ok())
        .filter_map(|handle| Some((buildings.get(&handle)?.name.clone(), handle)))
        .collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    let buttons: Vec<_> = entries
        .into_iter()
        .map(|(name, handle)| part_button(name, handle))
        .collect();
    commands.queue_spawn_scene(build_menu(buttons));
    Ok(())
}

fn build_menu(buttons: Vec<impl Scene>) -> impl Scene {
    bsn! {
        #BuildMenu
        Node {
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            width: px(200),
            max_height: percent(100),
        }
        BackgroundColor(PANEL_BACKGROUND)
        Children [
            @label("Buildings")
            --
            // Scrolling list
            Node {
                flex_direction: FlexDirection::Column,
                align_self: AlignSelf::Stretch,
                overflow: Overflow::scroll_y(),
            }
            Children [ {buttons} ]
        ]
    }
}

fn part_button(name: String, building: Handle<Building>) -> impl Scene {
    bsn! {
        Button
        Hovered
        Node {
            min_height: px(BUTTON_HEIGHT),
            max_height: px(BUTTON_HEIGHT),
            border: px(5),
        }
        BackgroundColor(NORMAL_BUTTON)
        BorderColor::from(Color::BLACK)
        Pickable { should_block_lower: false }
        PartButton(building)
        on(start_placement)
        Children [
            @label(name)
            Pickable { should_block_lower: false }
        ]
    }
}

/// UI text in the UI font.
pub fn label(text: impl Into<String>) -> impl Scene {
    let text = text.into();
    bsn! {
        Text(text)
        TextFont {
            font: FontSourceTemplate::Handle(UI_FONT),
            font_size: px(FONT_SIZE),
        }
        Label
    }
}

/// Start placing the building of a part button when it is activated.
fn start_placement(activate: On<Activate>, buttons: Query<&PartButton>, mut commands: Commands) {
    if let Ok(PartButton(building)) = buttons.get(activate.entity) {
        commands.trigger(StartPlacement(building.clone()));
    }
}

/// Restyle part buttons according to their hovered / pressed state.
fn update_button_appearance(
    mut buttons: Query<
        (&Hovered, Has<Pressed>, &mut BackgroundColor, &mut BorderColor),
        With<PartButton>,
    >,
) {
    for (hovered, pressed, mut color, mut border_color) in &mut buttons {
        let (background, border) = if pressed {
            (PRESSED_BUTTON, RED.into())
        } else if hovered.get() {
            (HOVERED_BUTTON, Color::WHITE)
        } else {
            (NORMAL_BUTTON, Color::BLACK)
        };
        color.set_if_neq(background.into());
        border_color.set_if_neq(BorderColor::all(border));
    }
}

/// Updates the scroll position of scrollable nodes in response to mouse input
fn update_scroll_position(
    mut mouse_wheel_events: MessageReader<MouseWheel>,
    hover_map: Res<HoverMap>,
    mut scrolled_node_query: Query<&mut ScrollPosition>,
) {
    for mouse_wheel_event in mouse_wheel_events.read() {
        let dy = match mouse_wheel_event.unit {
            MouseScrollUnit::Line => mouse_wheel_event.y * LINE_HEIGHT,
            MouseScrollUnit::Pixel => mouse_wheel_event.y,
        };

        for pointer_map in hover_map.values() {
            for entity in pointer_map.keys() {
                if let Ok(mut scroll_position) = scrolled_node_query.get_mut(*entity) {
                    scroll_position.y -= dy;
                }
            }
        }
    }
}

/// Whether the mouse pointer is currently over a UI node.
#[derive(SystemParam)]
pub struct PointerOverUi<'w, 's> {
    hover_map: Res<'w, HoverMap>,
    nodes: Query<'w, 's, (), With<Node>>,
}

impl PointerOverUi<'_, '_> {
    pub fn get(&self) -> bool {
        self.hover_map
            .get(&PointerId::Mouse)
            .is_some_and(|hits| hits.keys().any(|entity| self.nodes.contains(*entity)))
    }
}
