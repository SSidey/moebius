use anyhow::Result;

/// Primary port — driven by the CLI adapter to configure an AI provider.
pub trait UseAdapterPort {
    fn run(&self, adapter_name: &str) -> Result<()>;
}
