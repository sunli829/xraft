use serde::{Deserialize, Serialize};

use crate::{Entry, LogIndex, NodeId, NodeInfo, TermId};

pub type NetworkResult<T> = anyhow::Result<T>;

#[derive(Serialize, Deserialize)]
pub struct VoteRequest {
    pub term: TermId,
    pub candidate_id: NodeId,
    pub last_log_index: LogIndex,
    pub last_log_term: LogIndex,
}

#[derive(Serialize, Deserialize)]
pub struct VoteResponse {
    pub term: u64,
    pub vote_granted: bool,
}

#[derive(Serialize, Deserialize)]
pub struct AppendEntriesRequest<T> {
    pub term: TermId,
    pub leader_id: NodeId,
    pub prev_log_index: LogIndex,
    pub prev_log_term: TermId,
    pub leader_commit: LogIndex,
    pub entries: Vec<Entry<T>>,
}

#[derive(Serialize, Deserialize)]
pub struct AppendEntriesResponse {
    pub term: TermId,
    pub success: bool,
}

#[async_trait::async_trait]
pub trait Network<NodeDetail, Data>: Send + Sync + Unpin {
    async fn vote(
        &self,
        target: &NodeInfo<NodeDetail>,
        req: VoteRequest,
    ) -> NetworkResult<VoteResponse>;

    async fn append_entries(
        &self,
        target: &NodeInfo<NodeDetail>,
        req: AppendEntriesRequest<Data>,
    ) -> NetworkResult<AppendEntriesResponse>;
}
