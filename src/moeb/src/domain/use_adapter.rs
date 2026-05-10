use anyhow::Result;

pub struct UseAdapterService;

impl UseAdapterService {
    pub fn run(&self, adapter_name: &str) -> Result<()> {
        crate::commands::use_cmd::run(adapter_name)
    }
}
