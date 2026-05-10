use anyhow::Result;

/// Primary port — driven by the CLI adapter to produce a specification from raw input.
pub trait SpecPort {
    fn run(&self, input: &str) -> Result<()>;
}
