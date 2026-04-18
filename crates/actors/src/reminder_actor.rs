use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort};

use application::ports::{Notification, Notifier, ReminderRepository};
use domain::entities::Reminder;
use domain::services;

pub enum ReminderMsg {
    Create(String, DateTime<Utc>, RpcReplyPort<Result<Reminder, String>>),
    Delete(String, RpcReplyPort<Result<(), String>>),
    List(RpcReplyPort<Vec<Reminder>>),
    CheckDue,
}

pub struct ReminderActor {
    pub repo: Arc<dyn ReminderRepository>,
    pub notifier: Arc<dyn Notifier>,
}

impl Actor for ReminderActor {
    type Msg = ReminderMsg;
    type State = HashMap<String, Reminder>;
    type Arguments = ();

    async fn pre_start(
        &self,
        _: ActorRef<Self::Msg>,
        _: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        let all = self.repo.list().await.unwrap_or_default();
        let mut map = HashMap::with_capacity(all.len());
        for r in all {
            map.insert(r.id.clone(), r);
        }
        tracing::info!(
            count = map.len(),
            notifier = self.notifier.name(),
            "reminder actor: loaded reminders from db"
        );
        Ok(map)
    }

    async fn handle(
        &self,
        _: ActorRef<Self::Msg>,
        msg: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match msg {
            ReminderMsg::Create(text, remind_at, reply) => {
                let result = match services::create_reminder(text, remind_at) {
                    Ok(r) => match self.repo.upsert(&r).await {
                        Ok(()) => {
                            tracing::info!(id = %r.id, at = %r.remind_at, "reminder created");
                            state.insert(r.id.clone(), r.clone());
                            Ok(r)
                        }
                        Err(e) => Err(e.to_string()),
                    },
                    Err(e) => Err(e.to_string()),
                };
                let _ = reply.send(result);
            }
            ReminderMsg::Delete(id, reply) => {
                let result = match self.repo.delete(&id).await {
                    Ok(()) => {
                        state.remove(&id);
                        tracing::info!(%id, "reminder deleted");
                        Ok(())
                    }
                    Err(e) => Err(e.to_string()),
                };
                let _ = reply.send(result);
            }
            ReminderMsg::List(reply) => {
                let mut all: Vec<Reminder> = state.values().cloned().collect();
                all.sort_by(|a, b| a.remind_at.cmp(&b.remind_at));
                let _ = reply.send(all);
            }
            ReminderMsg::CheckDue => {
                let now = Utc::now();
                let due: Vec<Reminder> = state
                    .values()
                    .filter(|r| !r.fired && r.remind_at <= now)
                    .cloned()
                    .collect();
                if due.is_empty() {
                    return Ok(());
                }
                tracing::debug!(count = due.len(), "firing due reminders");
                for mut r in due {
                    let notif = Notification {
                        title: "Reminder".to_string(),
                        body: r.text.clone(),
                    };
                    if let Err(e) = self.notifier.notify(&notif).await {
                        tracing::warn!(id = %r.id, error = %e, "notify failed");
                        continue;
                    }
                    if let Err(e) = self.repo.set_fired(&r.id, true).await {
                        tracing::error!(id = %r.id, error = %e, "mark fired failed");
                        continue;
                    }
                    r.fired = true;
                    state.insert(r.id.clone(), r);
                }
            }
        }
        Ok(())
    }
}
