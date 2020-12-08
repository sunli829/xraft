use std::sync::Arc;

use fnv::FnvHashMap;
use serde::{Deserialize, Serialize};

pub type NodeId = u64;
pub type TermId = u64;
pub type LogIndex = u64;

#[derive(Debug, Serialize, Deserialize, Copy, Clone, Eq, PartialEq)]
pub enum Role {
    Follower,
    Candidate,
    Leader,
    NonVoter,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MemberShipConfig<N> {
    pub members: FnvHashMap<NodeId, Arc<N>>,
    pub non_voters: FnvHashMap<NodeId, Arc<N>>,
}

impl<N> Default for MemberShipConfig<N> {
    fn default() -> Self {
        Self {
            members: Default::default(),
            non_voters: Default::default(),
        }
    }
}

impl<N> MemberShipConfig<N> {
    pub(crate) fn other_members(&self, current: NodeId) -> impl Iterator<Item = (NodeId, Arc<N>)> {
        self.other_voter_members(current)
            .chain(self.non_voters.clone())
    }

    pub(crate) fn other_voter_members(
        &self,
        current: NodeId,
    ) -> impl Iterator<Item = (NodeId, Arc<N>)> {
        self.members
            .clone()
            .into_iter()
            .filter(move |(id, _)| *id != current)
    }

    pub(crate) fn member_info(&self, id: NodeId) -> Option<Arc<N>> {
        self.members
            .get(&id)
            .or_else(|| self.non_voters.get(&id))
            .cloned()
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub enum EntryDetail<N, D> {
    Normal(D),
    Blank,
    ChangeMemberShip(MemberShipConfig<N>),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Entry<N, D> {
    pub term: TermId,
    pub index: LogIndex,
    pub detail: EntryDetail<N, D>,
}

#[derive(Serialize, Deserialize)]
pub struct Metrics {
    pub id: NodeId,
    pub role: Role,
    pub current_term: TermId,
    pub last_log_index: LogIndex,
    pub last_applied: LogIndex,
    pub has_leader: bool,
    pub current_leader: NodeId,
}
