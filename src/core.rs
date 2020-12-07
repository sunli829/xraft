use std::sync::Arc;

use fnv::{FnvHashMap, FnvHashSet};
use futures::channel::{mpsc, oneshot};
use futures::future::BoxFuture;
use futures::task::AtomicWaker;

use crate::message::Message;
use crate::{
    AppendEntriesResponse, ClientError, Config, Network, NetworkResult, NodeId, NodeInfo, Role,
    Storage, VoteResponse,
};

type ReplySender = oneshot::Sender<Result<(), ClientError>>;

type AppendEntriesFuture =
    BoxFuture<'static, NetworkResult<(u64, u64, u64, AppendEntriesResponse)>>;

pub struct Core<N, D> {
    name: String,
    id: NodeId,
    config: Arc<Config>,
    storage: Arc<dyn Storage<D>>,
    network: Arc<dyn Network<N, D>>,
    rx: mpsc::Receiver<Message<D>>,
    members: Vec<NodeInfo<N>>,
    role: Role,
    voted_for: Option<NodeId>,
    voted: FnvHashSet<NodeId>,
    current_term: u64,
    last_log_term: u64,
    last_log_index: u64,
    commit_index: u64,
    leader: Option<NodeId>,
    pending_write: FnvHashMap<u64, Vec<ReplySender>>,
    heartbeat_timeout: Option<Delay>,
    election_timeout: Option<Delay>,
    next_index: FnvHashMap<NodeId, u64>,
    match_index: FnvHashMap<NodeId, u64>,
    vote_futures: OrderedGroup<NetworkResult<(u64, u64, VoteResponse)>>,
    append_entries_futures: Option<FnvHashMap<NodeId, AppendEntriesFuture>>,
    next_append_entries_futures: FnvHashMap<NodeId, bool>,
    buffered_writes: Vec<WriteAction>,
    buffered_reply: Vec<ReplySender>,
    waker: AtomicWaker,
}
