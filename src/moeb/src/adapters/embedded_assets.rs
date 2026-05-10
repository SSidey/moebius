use crate::assets::Assets;
use crate::ports::AssetPort;

pub struct EmbeddedAssetsAdapter;

impl AssetPort for EmbeddedAssetsAdapter {
    fn get(&self, name: &str) -> Option<Vec<u8>> {
        Assets::get(name).map(|f| f.data.into_owned())
    }
}
