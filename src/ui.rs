use bevy::{
    color::palettes::basic::*,
    input::mouse::{MouseScrollUnit, MouseWheel},
    picking::hover::HoverMap,
    prelude::*,
};

use crate::build::{BuildId, Building, setup_parts};
pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        //setup ui needs the parts list first
        app.add_systems(Startup, setup_ui.after(setup_parts));
        app.add_systems(Update, (update_scroll_position, button_system, update_building_list));
        app.insert_resource(FontHandle::default());
    }
}

const FONT_SIZE: f32 = 20.;
const LINE_HEIGHT: f32 = 21.;

#[derive(Component)]
pub struct PartButton {
    part_id: BuildId,
}

fn setup_ui(mut commands: Commands, asset_server: Res<AssetServer>, mut font: ResMut<FontHandle>) {
    font.0 = asset_server.load("fonts/FiraSans-Bold.ttf");
    // root node
    commands
        .spawn(Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            justify_content: JustifyContent::SpaceBetween,
            flex_direction: FlexDirection::Column,
            ..default()
        })
        .insert(Pickable::IGNORE)
        .with_children(|parent| {
            // container for all other examples
            parent
                .spawn(Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(5.),
                    height: Val::Percent(100.),
                    ..default()
                })
                .with_children(|parent| {
                    // vertical scroll
                    parent
                        .spawn((
                            Node {
                                flex_direction: FlexDirection::Column,
                                justify_content: JustifyContent::Center,
                                align_items: AlignItems::Center,
                                width: Val::Px(200.),
                                max_height: Val::Percent(100.),
                                ..default()
                            },
                            BackgroundColor(Color::srgb(0.10, 0.10, 0.10)),
                        ))
                        .with_children(|parent| {
                            // Title
                            parent.spawn((
                                Text::new("Vertically Scrolling List"),
                                TextFont {
                                    font: font.0.clone(),
                                    font_size: FONT_SIZE,
                                    ..default()
                                },
                                Label,
                            ));
                            // Scrolling list
                            parent.spawn((
                                Node {
                                    flex_direction: FlexDirection::Column,
                                    align_self: AlignSelf::Stretch,
                                    height: Val::Percent(90.),
                                    overflow: Overflow::scroll_y(), // n.b.
                                    ..default()
                                },
                                BuildingList,
                            ));
                        });
                });
        });
}
#[derive(Component)]
pub struct BuildingList;

#[derive(Resource, Default)]
pub struct FontHandle(pub Handle<Font>);

pub fn update_building_list(
    mut commands: Commands,
    mut events: EventReader<AssetEvent<Building>>,
    mut buildings: ResMut<Assets<Building>>,
    list_query: Single<Entity, With<BuildingList>>,
    font: Res<FontHandle>,
) {
    for ev in events.read() {
        if let AssetEvent::LoadedWithDependencies { id } = ev {
            commands.entity(*list_query).with_children(|parent| {
                // List items
                let building_handle = buildings.get_strong_handle(*id).unwrap();
                let building = buildings.get(*id).unwrap();
                parent
                    .spawn((
                        Button,
                        Node {
                            min_height: Val::Px(2. * LINE_HEIGHT),
                            max_height: Val::Px(2. * LINE_HEIGHT),
                            border: UiRect::all(Val::Px(5.0)),
                            ..default()
                        },
                        Pickable {
                            should_block_lower: false,
                            ..default()
                        },
                        PartButton {
                            part_id: BuildId(building_handle),
                        },
                    ))
                    .with_children(|parent| {
                        parent
                            .spawn((
                                Text(format!("Item {:}", building.name)),
                                TextFont {
                                    font: font.0.clone(),
                                    ..default()
                                },
                                Label,
                            ))
                            .insert(Pickable {
                                should_block_lower: false,
                                ..default()
                            });
                    });
            });
        }
    }
}

/// Updates the scroll position of scrollable nodes in response to mouse input
pub fn update_scroll_position(
    mut mouse_wheel_events: EventReader<MouseWheel>,
    hover_map: Res<HoverMap>,
    mut scrolled_node_query: Query<&mut ScrollPosition>,
    //keyboard_input: Res<ButtonInput<KeyCode>>,
) {
    for mouse_wheel_event in mouse_wheel_events.read() {
        let (_, dy) = match mouse_wheel_event.unit {
            MouseScrollUnit::Line => (
                mouse_wheel_event.x * LINE_HEIGHT,
                mouse_wheel_event.y * LINE_HEIGHT,
            ),
            MouseScrollUnit::Pixel => (mouse_wheel_event.x, mouse_wheel_event.y),
        };

        for (_pointer, pointer_map) in hover_map.iter() {
            for (entity, _hit) in pointer_map.iter() {
                if let Ok(mut scroll_position) = scrolled_node_query.get_mut(*entity) {
                    scroll_position.offset_y -= dy;
                }
            }
        }
    }
}

const NORMAL_BUTTON: Color = Color::srgb(0.15, 0.15, 0.15);
const HOVERED_BUTTON: Color = Color::srgb(0.25, 0.25, 0.25);
const PRESSED_BUTTON: Color = Color::srgb(0.35, 0.75, 0.35);

/// Change the button appearance when it is pressed.
fn button_system(
    mut commands: Commands,
    mut interaction_query: Query<
        (
            &Interaction,
            &mut BackgroundColor,
            &mut BorderColor,
            &PartButton,
        ),
        (Changed<Interaction>, With<Button>),
    >,
) {
    for (interaction, mut color, mut border_color, part_button) in &mut interaction_query {
        match *interaction {
            Interaction::Pressed => {
                *color = PRESSED_BUTTON.into();
                border_color.0 = RED.into();
                commands.spawn(part_button.part_id.clone());
            }
            Interaction::Hovered => {
                *color = HOVERED_BUTTON.into();
                border_color.0 = Color::WHITE;
            }
            Interaction::None => {
                *color = NORMAL_BUTTON.into();
                border_color.0 = Color::BLACK;
            }
        }
    }
}
