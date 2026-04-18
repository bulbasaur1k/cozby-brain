use std::sync::Arc;

use async_trait::async_trait;

use application::ports::{NotifyError, Notification, Notifier};

/// Fan-out notifier: delivers the same notification to several underlying
/// notifiers. Failures in one channel are logged but do not abort the rest.
pub struct CompositeNotifier {
    channels: Vec<Arc<dyn Notifier>>,
}

impl CompositeNotifier {
    pub fn new(channels: Vec<Arc<dyn Notifier>>) -> Self {
        Self { channels }
    }
}

#[async_trait]
impl Notifier for CompositeNotifier {
    fn name(&self) -> &str {
        "composite"
    }

    async fn notify(&self, n: &Notification) -> Result<(), NotifyError> {
        for ch in &self.channels {
            if let Err(e) = ch.notify(n).await {
                tracing::warn!(channel = ch.name(), error = %e, "notify channel failed");
            }
        }
        Ok(())
    }
}
