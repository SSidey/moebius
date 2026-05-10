use anyhow::Result;

/// Primary port — driven by the CLI adapter to implement the next step of a specification.
pub trait RunPort {
    fn run(&self, spec: &str) -> Result<()>;
}
