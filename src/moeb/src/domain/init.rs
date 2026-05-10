use anyhow::Result;

pub struct InitService;

impl InitService {
    pub fn run(&self) -> Result<()> {
        crate::commands::init::run()
    }
}
