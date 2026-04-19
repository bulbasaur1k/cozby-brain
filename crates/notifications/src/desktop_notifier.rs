//! Desktop notifier — native OS popups with sound.
//!
//! On macOS uses built-in Notification Center with a system sound
//! ("Glass" by default). On Linux uses libnotify / DBus. On Windows
//! uses the native toast API. All via `notify-rust`.
//!
//! The actual notify call is blocking (DBus/IOKit) so we offload it
//! to `tokio::task::spawn_blocking` to avoid stalling the async runtime.

use async_trait::async_trait;
use notify_rust::Notification as DesktopNotification;

use application::ports::{NotifyError, Notification, Notifier};

pub struct DesktopNotifier {
    app_name: String,
    sound: Option<String>,
}

impl DesktopNotifier {
    /// Default: app_name = "cozby-brain", sound = "Glass" (macOS built-in).
    pub fn new() -> Self {
        Self {
            app_name: "cozby-brain".to_string(),
            sound: Some("Glass".to_string()),
        }
    }

    pub fn with_app_name(mut self, name: impl Into<String>) -> Self {
        self.app_name = name.into();
        self
    }

    /// `None` — silent. On macOS valid sounds: Glass, Ping, Pop, Purr, Blow, …
    pub fn with_sound(mut self, sound: Option<String>) -> Self {
        self.sound = sound;
        self
    }
}

impl Default for DesktopNotifier {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Notifier for DesktopNotifier {
    fn name(&self) -> &str {
        "desktop"
    }

    async fn notify(&self, n: &Notification) -> Result<(), NotifyError> {
        let title = n.title.clone();
        let body = n.body.clone();
        let app = self.app_name.clone();
        let sound = self.sound.clone();

        tokio::task::spawn_blocking(move || {
            let mut desk = DesktopNotification::new();
            desk.appname(&app)
                .summary(&title)
                .body(&body)
                .timeout(notify_rust::Timeout::Milliseconds(10_000));
            if let Some(s) = sound.as_deref() {
                desk.sound_name(s);
            }
            desk.show()
                .map(|_| ())
                .map_err(|e| NotifyError::Other(format!("desktop notify: {e}")))
        })
        .await
        .map_err(|e| NotifyError::Other(format!("join: {e}")))?
    }
}
