use anyhow::Result;

pub trait ListAdaptersPort {
    fn run(&self) -> Result<()>;
}
