use std::ops::Deref;

use serde::{Deserialize, Serialize};

pub type NodeId = u64;
pub type TermId = u64;
pub type LogIndex = u64;

#[derive(Debug, Serialize, Deserialize, Copy, Clone)]
pub enum Role {
    Follower,
    Candidate,
    Leader,
    NonVoter,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum EntryDetail<D> {
    Normal(D),
    Blank,
    ChangeMemberShip,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Entry<D> {
    pub term: TermId,
    pub index: LogIndex,
    pub detail: EntryDetail<D>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NodeInfo<N = ()> {
    pub id: NodeId,
    pub detail: N,
}

impl<N> Deref for NodeInfo<N> {
    type Target = N;

    fn deref(&self) -> &Self::Target {
        &self.detail
    }
}
