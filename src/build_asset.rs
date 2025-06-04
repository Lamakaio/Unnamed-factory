use bevy::{
    asset::{AssetLoader, LoadContext},
    prelude::*,
};
use serde::Deserialize;

use crate::{
    build::{Building, BuildingType},
    map::PatchOp,
};

pub struct BuildAssetPlugin;

impl Plugin for BuildAssetPlugin {
    fn build(&self, app: &mut App) {
        app.init_asset::<Building>()
            .init_asset_loader::<BuildingLoader>();
    }
}

#[derive(Deserialize)]
enum BuildingTypFile {
    Zone { color: LinearRgba },
    Single { model: String, scale: f32 },
    Tool { op: PatchOp, color: LinearRgba },
}
#[derive(Deserialize)]
struct BuildingFile {
    name: String,
    size: (u64, u64),
    typ: BuildingTypFile,
    #[serde(default)]
    script: String,
}

#[derive(Default)]
pub struct BuildingLoader;

impl AssetLoader for BuildingLoader {
    type Asset = Building;

    type Settings = ();

    type Error = anyhow::Error;

    async fn load(
        &self,
        reader: &mut dyn bevy::asset::io::Reader,
        _settings: &Self::Settings,
        load_context: &mut LoadContext<'_>,
    ) -> Result<Self::Asset, Self::Error> {
        let mut bytes = Vec::new();

        reader.read_to_end(&mut bytes).await?;
        let parsed_build_file = ron::de::from_bytes::<BuildingFile>(&bytes)?;

        let typ = match parsed_build_file.typ {
            BuildingTypFile::Zone { color } => BuildingType::Zone {
                color: color.into(),
            },
            BuildingTypFile::Single { model, scale } => BuildingType::Single {
                model: load_context.load(GltfAssetLabel::Scene(0).from_asset(model)),
                scale
            },
            BuildingTypFile::Tool { op, color } => BuildingType::Tool {
                op,
                color: color.into(),
            },
        };
        let script = if parsed_build_file.script.is_empty() {
            None
        } else {
            Some(load_context.load(parsed_build_file.script))
        };

        Ok(Building {
            typ,
            name: parsed_build_file.name,
            size: parsed_build_file.size,
            script,
        })
    }

    fn extensions(&self) -> &[&str] {
        &["bconf"]
    }
}
