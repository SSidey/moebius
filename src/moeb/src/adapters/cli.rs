use anyhow::Result;
use std::sync::Arc;

use crate::adapters::anthropic::AnthropicAdapter;
use crate::adapters::openai::OpenAiAdapter;
use crate::config::MoebConfig;
use crate::domain::{init::InitService, run::RunService, spec::SpecService, use_adapter::UseAdapterService};
use crate::ports::{AdapterManagementPort, AiPort, InitPort, ListAdaptersPort, RunPort, SpecPort, UseAdapterPort};

pub struct CliAdapter;

impl InitPort for CliAdapter {
    fn run(&self) -> Result<()> {
        InitService.run()
    }
}

impl UseAdapterPort for CliAdapter {
    fn run(&self, adapter_name: &str) -> Result<()> {
        UseAdapterService.run(adapter_name)
    }
}

impl ListAdaptersPort for CliAdapter {
    fn run(&self) -> Result<()> {
        crate::commands::adapters::run()
    }
}

impl AdapterManagementPort for CliAdapter {
    fn configure(&self, adapter: &str, key: &str, value: &str) -> Result<()> {
        crate::commands::adapter_management::configure(adapter, key, value)
    }

    fn release(&self, adapter: &str) -> Result<()> {
        crate::commands::adapter_management::release(adapter)
    }
}

impl SpecPort for CliAdapter {
    fn run(&self, input: &str) -> Result<()> {
        let ai = resolve_ai_adapter()?;
        SpecService::new(ai).run(input)
    }
}

impl RunPort for CliAdapter {
    fn run(&self, spec: &str) -> Result<()> {
        let ai = resolve_ai_adapter()?;
        RunService::new(ai).run(spec)
    }
}

fn resolve_ai_adapter() -> Result<Arc<dyn AiPort>> {
    let config = MoebConfig::load()?;
    let adapter_name = config.active_adapter.as_deref().unwrap_or("");
    if adapter_name.is_empty() {
        anyhow::bail!("No adapter configured. Run `moeb use <adapter>` first.");
    }
    match adapter_name {
        "openai" => Ok(Arc::new(OpenAiAdapter::from_secrets_and_config()?)),
        "anthropic" => Ok(Arc::new(AnthropicAdapter::from_secrets_and_config()?)),
        other => anyhow::bail!(
            "Adapter '{}' is configured but not recognised. Run `moeb use <adapter>` to reconfigure.",
            other
        ),
    }
}
