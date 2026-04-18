use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort};

use application::ports::TodoRepository;
use domain::entities::Todo;
use domain::services;

pub enum TodoMsg {
    Create(String, Option<DateTime<Utc>>, RpcReplyPort<Result<Todo, String>>),
    Complete(String, RpcReplyPort<Result<Todo, String>>),
    Delete(String, RpcReplyPort<Result<(), String>>),
    List(RpcReplyPort<Vec<Todo>>),
}

pub struct TodoActor {
    pub repo: Arc<dyn TodoRepository>,
}

impl Actor for TodoActor {
    type Msg = TodoMsg;
    type State = HashMap<String, Todo>;
    type Arguments = ();

    async fn pre_start(
        &self,
        _: ActorRef<Self::Msg>,
        _: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        let all = self.repo.list().await.unwrap_or_default();
        let mut map = HashMap::with_capacity(all.len());
        for t in all {
            map.insert(t.id.clone(), t);
        }
        tracing::info!(count = map.len(), "todo actor: loaded todos from db");
        Ok(map)
    }

    async fn handle(
        &self,
        _: ActorRef<Self::Msg>,
        msg: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match msg {
            TodoMsg::Create(title, due_at, reply) => {
                let result = match services::create_todo(title, due_at) {
                    Ok(todo) => match self.repo.upsert(&todo).await {
                        Ok(()) => {
                            tracing::info!(id = %todo.id, title = %todo.title, "todo created");
                            state.insert(todo.id.clone(), todo.clone());
                            Ok(todo)
                        }
                        Err(e) => Err(e.to_string()),
                    },
                    Err(e) => Err(e.to_string()),
                };
                let _ = reply.send(result);
            }
            TodoMsg::Complete(id, reply) => {
                let result = match state.get(&id).cloned() {
                    None => Err(format!("not found: {id}")),
                    Some(t) if t.done => Ok(t),
                    Some(t) => {
                        let done = t.complete();
                        match self.repo.upsert(&done).await {
                            Ok(()) => {
                                tracing::info!(%id, "todo completed");
                                state.insert(done.id.clone(), done.clone());
                                Ok(done)
                            }
                            Err(e) => Err(e.to_string()),
                        }
                    }
                };
                let _ = reply.send(result);
            }
            TodoMsg::Delete(id, reply) => {
                let result = match self.repo.delete(&id).await {
                    Ok(()) => {
                        state.remove(&id);
                        tracing::info!(%id, "todo deleted");
                        Ok(())
                    }
                    Err(e) => Err(e.to_string()),
                };
                let _ = reply.send(result);
            }
            TodoMsg::List(reply) => {
                let mut all: Vec<Todo> = state.values().cloned().collect();
                all.sort_by(|a, b| {
                    a.done
                        .cmp(&b.done)
                        .then_with(|| a.due_at.unwrap_or(a.created_at).cmp(&b.due_at.unwrap_or(b.created_at)))
                });
                let _ = reply.send(all);
            }
        }
        Ok(())
    }
}
