use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;
use xraft::{
    AppendEntriesRequest, AppendEntriesResponse, Network, NetworkResult, NodeId, VoteRequest,
    VoteResponse,
};

use super::{Action, Member};

#[derive(Default)]
pub struct TestNetwork {
    pub nodes: Arc<RwLock<HashMap<NodeId, Member>>>,
}

#[async_trait::async_trait]
impl Network<(), Action> for TestNetwork {
    async fn vote(
        &self,
        target: u64,
        _target_info: &(),
        req: VoteRequest,
    ) -> NetworkResult<VoteResponse> {
        let node = self.nodes.read().get(&target).cloned().unwrap();
        Ok(node.raft.vote(req).await?)
    }

    async fn append_entries(
        &self,
        target: u64,
        _target_info: &(),
        req: AppendEntriesRequest<(), Action>,
    ) -> NetworkResult<AppendEntriesResponse> {
        let node = self.nodes.read().get(&target).cloned().unwrap();
        Ok(node.raft.append_entries(req).await?)
    }
}
