use std::sync::Arc;

use fnv::{FnvHashMap, FnvHashSet};
use futures::channel::{mpsc, oneshot};
use futures::future::BoxFuture;
use futures::task::AtomicWaker;

use crate::message::Message;
use crate::ordered_group::OrderedGroup;
use crate::runtime::Delay;
use crate::{
    AppendEntriesResponse, ClientError, Config, Network, NetworkResult, NodeId, NodeInfo,
    RaftError, Result, Role, Storage, VoteResponse,
};

const MESSAGE_CHANNEL_SIZE: usize = 32;

type ReplySender = oneshot::Sender<Result<(), ClientError>>;

type AppendEntriesFuture =
    BoxFuture<'static, NetworkResult<(u64, u64, u64, AppendEntriesResponse)>>;

pub struct Core<N, D> {
    name: String,
    node: Arc<NodeInfo<N>>,
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
    buffered_writes: Vec<D>,
    buffered_reply: Vec<ReplySender>,
    waker: AtomicWaker,
}

impl<N, D: 'static> Core<N, D> {
    pub fn new(
        name: impl Into<String>,
        node: NodeInfo<N>,
        config: Arc<Config>,
        storage: Arc<dyn Storage<D>>,
        network: Arc<dyn Network<N, D>>,
    ) -> Result<(Self, mpsc::Sender<Message<D>>)> {
        let (tx, rx) = mpsc::channel(MESSAGE_CHANNEL_SIZE);
        let name = name.into();

        let mut core = Self {
            name,
            node: Arc::new(node),
            config,
            storage,
            network,
            rx,
            members: Vec::new(),
            role: Role::NonVoter,
            voted_for: None,
            voted: Default::default(),
            current_term: 0,
            last_log_term: 0,
            last_log_index: 0,
            commit_index: 0,
            leader: None,
            pending_write: Default::default(),
            heartbeat_timeout: None,
            election_timeout: None,
            next_index: Default::default(),
            match_index: Default::default(),
            vote_futures: Default::default(),
            append_entries_futures: Some(Default::default()),
            next_append_entries_futures: Default::default(),
            buffered_writes: Vec::new(),
            buffered_reply: Vec::new(),
            waker: AtomicWaker::new(),
        };

        let initial_state = core
            .storage
            .get_initial_state()
            .map_err(RaftError::Storage)?;
        core.last_log_index = initial_state.last_log_index;
        core.last_log_term = initial_state.last_log_term;

        if let Some(hard_state) = initial_state.hard_state {
            core.current_term = hard_state.current_term;
            core.voted_for = hard_state.voted_for;
            core.role = Role::Follower;
        }

        debug!(
            target: "tdb-raft",
            name = %core.name,
            id = %core.node.id,
            last_log_index = %core.last_log_index,
            last_log_term = %core.last_log_term,
            current_term = %core.current_term,
            last_applied = %core.storage.last_applied().map_err(RaftError::Storage)?,
            role = ?core.role,
            "Initialize raft",
        );

        Ok((core, tx))
    }
}
