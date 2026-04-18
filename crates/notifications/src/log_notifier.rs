use async_trait::async_trait;

use application::ports::{NotifyError, Notification, Notifier};

pub struct LogNotifier;

#[async_trait]
impl Notifier for LogNotifier {
    fn name(&self) -> &str {
        "log"
    }

    async fn notify(&self, n: &Notification) -> Result<(), NotifyError> {
        tracing::info!(target: "notify", title = %n.title, body = %n.body, "🔔 notification");
        Ok(())
    }
}
