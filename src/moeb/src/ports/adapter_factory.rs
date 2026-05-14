use std::sync::Arc;
use anyhow::Result;

use crate::ports::AiPort;
use crate::trace::TraceContext;

/// Primary port — called by the domain to obtain a traced AI adapter.
/// Concrete implementations live in the adapters layer.
pub trait AdapterFactoryPort: Send + Sync {
    fn build(&self, trace: Arc<TraceContext>) -> Result<Arc<dyn AiPort>>;
}
