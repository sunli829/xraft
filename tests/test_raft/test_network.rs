use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use parking_lot::RwLock;
use xraft::{
    AppendEntriesRequest, AppendEntriesResponse, Network, NetworkResult, NodeId, VoteRequest,
    VoteResponse,
};

use super::{Action, Node};

#[derive(Default)]
pub struct TestNetwork {
    pub nodes: Arc<RwLock<BTreeMap<NodeId, Node>>>,
    pub isolated_nodes: Arc<RwLock<BTreeSet<NodeId>>>,
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
        anyhow::ensure!(
            !self.isolated_nodes.read().contains(&target),
            "Node '{}' is isolated",
            target
        );
        Ok(node.raft.vote(req).await?)
    }

    async fn append_entries(
        &self,
        target: u64,
        _target_info: &(),
        req: AppendEntriesRequest<(), Action>,
    ) -> NetworkResult<AppendEntriesResponse> {
        let node = self.nodes.read().get(&target).cloned().unwrap();
        anyhow::ensure!(
            !self.isolated_nodes.read().contains(&target),
            "Node '{}' is isolated",
            target
        );
        Ok(node.raft.append_entries(req).await?)
    }
}
