use async_trait::async_trait;

use application::ports::{NotifyError, Notification, Notifier};

/// Mock notifier that writes straight to stdout.
pub struct StdoutNotifier;

#[async_trait]
impl Notifier for StdoutNotifier {
    fn name(&self) -> &str {
        "stdout"
    }

    async fn notify(&self, n: &Notification) -> Result<(), NotifyError> {
        println!("🔔 [{}] {}", n.title, n.body);
        Ok(())
    }
}
