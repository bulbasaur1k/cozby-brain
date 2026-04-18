use std::collections::HashMap;
use std::sync::Arc;

use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort};

use application::ports::NoteRepository;
use domain::entities::Note;
use domain::services;

pub enum NoteMsg {
    Create(String, String, Vec<String>, RpcReplyPort<Result<Note, String>>),
    Update(String, String, String, Vec<String>, RpcReplyPort<Result<Note, String>>),
    Delete(String, RpcReplyPort<Result<(), String>>),
    Get(String, RpcReplyPort<Option<Note>>),
    List(RpcReplyPort<Vec<Note>>),
    Search(String, RpcReplyPort<Vec<Note>>),
}

pub struct NoteActor {
    pub repo: Arc<dyn NoteRepository>,
}

impl Actor for NoteActor {
    type Msg = NoteMsg;
    type State = HashMap<String, Note>;
    type Arguments = ();

    async fn pre_start(
        &self,
        _: ActorRef<Self::Msg>,
        _: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        let all = self.repo.list().await.unwrap_or_default();
        let mut map = HashMap::with_capacity(all.len());
        for n in all {
            map.insert(n.id.clone(), n);
        }
        tracing::info!(count = map.len(), "note actor: loaded notes from db");
        Ok(map)
    }

    async fn handle(
        &self,
        _: ActorRef<Self::Msg>,
        msg: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match msg {
            NoteMsg::Create(title, content, tags, reply) => {
                let result = match services::create_note(title, content, tags) {
                    Ok(note) => match self.repo.upsert(&note).await {
                        Ok(()) => {
                            tracing::info!(id = %note.id, title = %note.title, "note created");
                            state.insert(note.id.clone(), note.clone());
                            Ok(note)
                        }
                        Err(e) => {
                            tracing::error!(error = %e, "note create: db error");
                            Err(e.to_string())
                        }
                    },
                    Err(e) => Err(e.to_string()),
                };
                let _ = reply.send(result);
            }
            NoteMsg::Update(id, title, content, tags, reply) => {
                let existing = state.get(&id).cloned();
                let result = match existing {
                    None => Err(format!("not found: {id}")),
                    Some(n) => match services::update_note(n, title, content, tags) {
                        Ok(updated) => match self.repo.upsert(&updated).await {
                            Ok(()) => {
                                tracing::info!(id = %updated.id, "note updated");
                                state.insert(updated.id.clone(), updated.clone());
                                Ok(updated)
                            }
                            Err(e) => Err(e.to_string()),
                        },
                        Err(e) => Err(e.to_string()),
                    },
                };
                let _ = reply.send(result);
            }
            NoteMsg::Delete(id, reply) => {
                let result = match self.repo.delete(&id).await {
                    Ok(()) => {
                        tracing::info!(%id, "note deleted");
                        state.remove(&id);
                        Ok(())
                    }
                    Err(e) => Err(e.to_string()),
                };
                let _ = reply.send(result);
            }
            NoteMsg::Get(id, reply) => {
                let _ = reply.send(state.get(&id).cloned());
            }
            NoteMsg::List(reply) => {
                let mut all: Vec<Note> = state.values().cloned().collect();
                all.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
                let _ = reply.send(all);
            }
            NoteMsg::Search(q, reply) => {
                let q_lower = q.to_lowercase();
                let mut matches: Vec<Note> = state
                    .values()
                    .filter(|n| {
                        n.title.to_lowercase().contains(&q_lower)
                            || n.content.to_lowercase().contains(&q_lower)
                    })
                    .cloned()
                    .collect();
                matches.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
                let _ = reply.send(matches);
            }
        }
        Ok(())
    }
}
