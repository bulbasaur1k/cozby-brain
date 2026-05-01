use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort};

use application::ports::{Notification, Notifier, ReminderRepository};
use domain::entities::Reminder;
use domain::recurrence;
use domain::services;

pub enum ReminderMsg {
    /// (text, remind_at, recurrence_rule, reply). recurrence_rule = None для
    /// одноразового напоминания.
    Create(
        String,
        DateTime<Utc>,
        Option<String>,
        RpcReplyPort<Result<Reminder, String>>,
    ),
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
            ReminderMsg::Create(text, remind_at, recurrence, reply) => {
                let result = match services::create_reminder(text, remind_at) {
                    Ok(r) => {
                        let r = r.with_recurrence(recurrence.filter(|s| !s.trim().is_empty()));
                        match self.repo.upsert(&r).await {
                            Ok(()) => {
                                tracing::info!(
                                    id = %r.id,
                                    at = %r.remind_at,
                                    recurring = r.is_recurring(),
                                    "reminder created"
                                );
                                state.insert(r.id.clone(), r.clone());
                                Ok(r)
                            }
                            Err(e) => Err(e.to_string()),
                        }
                    }
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
                // Рекуррентные напоминания не помечаются `fired=true` — они
                // живут вечно, поэтому фильтр на due использует «не fired
                // ИЛИ есть recurrence».
                let due: Vec<Reminder> = state
                    .values()
                    .filter(|r| r.remind_at <= now && (!r.fired || r.is_recurring()))
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

                    // Рекуррентное → пересчитываем remind_at, fired остаётся false.
                    // Иначе одноразово помечаем fired.
                    if let Some(rule_str) = r.recurrence.clone() {
                        match recurrence::parse(&rule_str)
                            .ok()
                            .and_then(|rule| recurrence::next_after(r.remind_at, &rule, now))
                        {
                            Some(next) => {
                                r.remind_at = next;
                                r.fired = false;
                                if let Err(e) = self.repo.upsert(&r).await {
                                    tracing::error!(id = %r.id, error = %e, "upsert recurring failed");
                                    continue;
                                }
                                tracing::info!(
                                    id = %r.id,
                                    next = %r.remind_at,
                                    rule = %rule_str,
                                    "recurring reminder advanced"
                                );
                                state.insert(r.id.clone(), r);
                            }
                            None => {
                                // Невалидное правило / выпадение за горизонт —
                                // деградируем до одноразового, чтобы не зацикливаться.
                                tracing::warn!(
                                    id = %r.id,
                                    rule = %rule_str,
                                    "recurrence invalid or exhausted, marking fired"
                                );
                                if let Err(e) = self.repo.set_fired(&r.id, true).await {
                                    tracing::error!(id = %r.id, error = %e, "mark fired failed");
                                    continue;
                                }
                                r.fired = true;
                                state.insert(r.id.clone(), r);
                            }
                        }
                    } else {
                        if let Err(e) = self.repo.set_fired(&r.id, true).await {
                            tracing::error!(id = %r.id, error = %e, "mark fired failed");
                            continue;
                        }
                        r.fired = true;
                        state.insert(r.id.clone(), r);
                    }
                }
            }
        }
        Ok(())
    }
}
