use anyhow::Result;

pub trait AdapterManagementPort {
    fn configure(&self, adapter: &str, key: &str, value: &str) -> Result<()>;
    fn release(&self, adapter: &str) -> Result<()>;
}
