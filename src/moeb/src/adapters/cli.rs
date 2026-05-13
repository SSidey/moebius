use anyhow::Result;

use crate::domain::{init::InitService, run::RunService, spec::SpecService, use_adapter::UseAdapterService};
use crate::ports::{AdapterManagementPort, InitPort, ListAdaptersPort, RunPort, SpecPort, UseAdapterPort};
use crate::trace::FileContentMode;

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
    fn run(&self, input: &str, file_content_mode: FileContentMode) -> Result<()> {
        SpecService::from_config().run(input, file_content_mode)
    }
}

impl RunPort for CliAdapter {
    fn run(&self, spec: &str, file_content_mode: FileContentMode) -> Result<()> {
        RunService::from_config().run(spec, file_content_mode)
    }
}
