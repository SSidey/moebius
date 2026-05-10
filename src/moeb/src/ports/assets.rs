/// Secondary port — implemented by asset providers (e.g. binary-embedded defaults).
/// The domain calls this port to retrieve default harness files.
pub trait AssetPort: Send + Sync {
    fn get(&self, name: &str) -> Option<Vec<u8>>;
}
