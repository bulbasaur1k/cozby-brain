//! NodeActor — owns composite entities («узлы») and their member links.
//!
//! Узел связывает несколько записей (note/todo/reminder/doc_page/link) в один
//! смысловой объект. Целевой кейс — дежурство/инцидент. Write-through: валидация
//! через domain::services → repo → кэш. Members не кэшируются (читаются из БД).

use std::collections::HashMap;
use std::sync::Arc;

use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort};

use application::ports::NodeRepository;
use domain::entities::{Node, NodeMember};
use domain::services;

pub enum NodeMsg {
    Create(
        String,      // kind
        String,      // title
        Vec<String>, // tags
        RpcReplyPort<Result<Node, String>>,
    ),
    Delete(String, RpcReplyPort<Result<(), String>>),
    UpdateSummary(String, String, RpcReplyPort<Result<Node, String>>),
    SetStatus(String, String, RpcReplyPort<Result<Node, String>>),
    SetMetadata(String, serde_json::Value, RpcReplyPort<Result<Node, String>>),
    Get(String, RpcReplyPort<Option<Node>>),
    List(RpcReplyPort<Vec<Node>>),
    ListByKind(String, RpcReplyPort<Vec<Node>>),
    AddMember(
        String, // node_id
        String, // member_kind
        String, // member_id (для 'link' — URL)
        String, // role
        String, // label
        RpcReplyPort<Result<NodeMember, String>>,
    ),
    RemoveMember(String, RpcReplyPort<Result<(), String>>),
    ListMembers(String, RpcReplyPort<Vec<NodeMember>>),
    FindForMember(String, String, RpcReplyPort<Vec<Node>>),
}

pub struct NodeActor {
    pub repo: Arc<dyn NodeRepository>,
}

impl Actor for NodeActor {
    type Msg = NodeMsg;
    /// Cache of node_id → Node. Members are not cached (read from db on demand).
    type State = HashMap<String, Node>;
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
        tracing::info!(count = map.len(), "node actor: loaded nodes from db");
        Ok(map)
    }

    async fn handle(
        &self,
        _: ActorRef<Self::Msg>,
        msg: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match msg {
            NodeMsg::Create(kind, title, tags, reply) => {
                let result = match services::create_node(kind, title, tags) {
                    Ok(node) => match self.repo.upsert(&node).await {
                        Ok(()) => {
                            tracing::info!(id = %node.id, kind = %node.kind, title = %node.title, "node created");
                            state.insert(node.id.clone(), node.clone());
                            Ok(node)
                        }
                        Err(e) => Err(e.to_string()),
                    },
                    Err(e) => Err(e.to_string()),
                };
                let _ = reply.send(result);
            }
            NodeMsg::Delete(id, reply) => {
                let result = match self.repo.delete(&id).await {
                    Ok(()) => {
                        tracing::info!(%id, "node deleted");
                        state.remove(&id);
                        Ok(())
                    }
                    Err(e) => Err(e.to_string()),
                };
                let _ = reply.send(result);
            }
            NodeMsg::UpdateSummary(id, summary, reply) => {
                let _ = reply.send(self.mutate(state, &id, |n| n.summary = summary).await);
            }
            NodeMsg::SetStatus(id, status, reply) => {
                let _ = reply.send(self.mutate(state, &id, |n| n.status = status).await);
            }
            NodeMsg::SetMetadata(id, metadata, reply) => {
                let _ = reply.send(self.mutate(state, &id, |n| n.metadata = metadata).await);
            }
            NodeMsg::Get(id, reply) => {
                let _ = reply.send(state.get(&id).cloned());
            }
            NodeMsg::List(reply) => {
                let mut all: Vec<Node> = state.values().cloned().collect();
                all.sort_by_key(|n| std::cmp::Reverse(n.updated_at));
                let _ = reply.send(all);
            }
            NodeMsg::ListByKind(kind, reply) => {
                let mut all: Vec<Node> =
                    state.values().filter(|n| n.kind == kind).cloned().collect();
                all.sort_by_key(|n| std::cmp::Reverse(n.updated_at));
                let _ = reply.send(all);
            }
            NodeMsg::AddMember(node_id, member_kind, member_id, role, label, reply) => {
                let result = if !state.contains_key(&node_id) {
                    Err(format!("node not found: {node_id}"))
                } else {
                    let member =
                        NodeMember::new(node_id, member_kind, member_id, role, label);
                    match self.repo.add_member(&member).await {
                        Ok(()) => {
                            tracing::info!(
                                node = %member.node_id,
                                kind = %member.member_kind,
                                member = %member.member_id,
                                "node member added"
                            );
                            Ok(member)
                        }
                        Err(e) => Err(e.to_string()),
                    }
                };
                let _ = reply.send(result);
            }
            NodeMsg::RemoveMember(id, reply) => {
                let result = self.repo.remove_member(&id).await.map_err(|e| e.to_string());
                let _ = reply.send(result);
            }
            NodeMsg::ListMembers(node_id, reply) => {
                let members = self.repo.list_members(&node_id).await.unwrap_or_default();
                let _ = reply.send(members);
            }
            NodeMsg::FindForMember(member_kind, member_id, reply) => {
                let nodes = self
                    .repo
                    .find_nodes_for_member(&member_kind, &member_id)
                    .await
                    .unwrap_or_default();
                let _ = reply.send(nodes);
            }
        }
        Ok(())
    }
}

impl NodeActor {
    /// Apply an in-place mutation to a node, persist (write-through), update cache.
    async fn mutate(
        &self,
        state: &mut HashMap<String, Node>,
        id: &str,
        f: impl FnOnce(&mut Node),
    ) -> Result<Node, String> {
        let Some(mut node) = state.get(id).cloned() else {
            return Err(format!("node not found: {id}"));
        };
        f(&mut node);
        node.touch();
        match self.repo.upsert(&node).await {
            Ok(()) => {
                state.insert(node.id.clone(), node.clone());
                Ok(node)
            }
            Err(e) => Err(e.to_string()),
        }
    }
}
