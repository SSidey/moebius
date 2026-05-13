use anyhow::Result;

use crate::trace::FileContentMode;

/// Primary port — driven by the CLI adapter to implement the next step of a specification.
pub trait RunPort {
    fn run(&self, spec: &str, file_content_mode: FileContentMode) -> Result<()>;
}
