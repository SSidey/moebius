use anyhow::Result;

/// Primary port — driven by the CLI adapter to initialise a project harness.
pub trait InitPort {
    fn run(&self) -> Result<()>;
}
