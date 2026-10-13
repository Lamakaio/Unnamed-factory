//! In-game UI: the loading text, built with BSN scenes.

use bevy::{
    ecs::system::SystemParam,
    picking::{hover::HoverMap, pointer::PointerId},
    prelude::*,
    text::FontSourceTemplate,
};

use crate::AppState;

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(AppState::Loading), spawn_loading_text);
    }
}

/// Font used by all UI text.
pub const UI_FONT: &str = "fonts/FiraSans-Bold.ttf";
const FONT_SIZE: f32 = 20.;

/// Centered text shown while the world is generated.
fn spawn_loading_text(mut commands: Commands) {
    commands
        .queue_spawn_scene(bsn! {
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
            }
            Children [ @label("Generating world...") ]
        })
        .insert(DespawnOnExit(AppState::Loading));
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
