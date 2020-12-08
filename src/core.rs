use std::cmp::Reverse;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use fnv::{FnvHashMap, FnvHashSet};
use futures::channel::{mpsc, oneshot};
use futures::future::BoxFuture;
use futures::task::AtomicWaker;
use futures::{Future, FutureExt, StreamExt};
use rand::prelude::*;
use smallvec::alloc::collections::BTreeMap;
use smallvec::SmallVec;

use crate::message::Message;
use crate::ordered_group::OrderedGroup;
use crate::runtime::Delay;
use crate::{
    AppendEntriesRequest, AppendEntriesResponse, Config, Entry, EntryDetail, HardState,
    MemberShipConfig, Metrics, Network, NetworkResult, NodeId, RaftError, Result, Role, Storage,
    VoteRequest, VoteResponse,
};

const MESSAGE_CHANNEL_SIZE: usize = 32;

type ReplySender = oneshot::Sender<Result<()>>;

type AppendEntriesFuture =
    BoxFuture<'static, Result<(u64, u64, u64, AppendEntriesResponse), anyhow::Error>>;

pub struct Core<N, D> {
    name: String,
    id: NodeId,
    config: Arc<Config>,
    storage: Arc<dyn Storage<N, D>>,
    network: Arc<dyn Network<N, D>>,
    rx: mpsc::Receiver<Message<N, D>>,
    membership: MemberShipConfig<N>,
    role: Role,
    voted_for: Option<NodeId>,
    voted: FnvHashSet<NodeId>,
    current_term: u64,
    last_log_term: u64,
    last_log_index: u64,
    commit_index: u64,
    leader: Option<NodeId>,
    pending_write: BTreeMap<u64, Vec<ReplySender>>,
    heartbeat_timeout: Option<Delay>,
    election_timeout: Option<Delay>,
    next_index: FnvHashMap<NodeId, u64>,
    match_index: FnvHashMap<NodeId, u64>,
    vote_futures: OrderedGroup<NetworkResult<(u64, u64, VoteResponse)>>,
    append_entries_futures: Option<FnvHashMap<NodeId, AppendEntriesFuture>>,
    next_append_entries_futures: FnvHashMap<NodeId, bool>,
    waker: AtomicWaker,
}

impl<N, D> Core<N, D>
where
    N: Send + Sync + Unpin + 'static,
    D: Send + Unpin + 'static,
{
    pub fn new(
        name: impl Into<String>,
        id: NodeId,
        config: Arc<Config>,
        storage: Arc<dyn Storage<N, D>>,
        network: Arc<dyn Network<N, D>>,
    ) -> Result<(Self, mpsc::Sender<Message<N, D>>)> {
        let (tx, rx) = mpsc::channel(MESSAGE_CHANNEL_SIZE);
        let name = name.into();

        let mut core = Self {
            name,
            id,
            config,
            storage,
            network,
            rx,
            membership: Default::default(),
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
        }

        if core.last_log_index > 0 {
            core.role = Role::Follower;
            core.reset_election_timeout();
        }

        debug!(
            name = core.name.as_str(),
            id = core.id,
            last_log_index = core.last_log_index,
            last_log_term = core.last_log_term,
            current_term = core.current_term,
            last_applied = core.storage.last_applied().map_err(RaftError::Storage)?,
            role = ?core.role,
            "Initialize raft",
        );

        Ok((core, tx))
    }

    fn reset_heartbeat_timeout(&mut self) {
        self.heartbeat_timeout = Some(Delay::new(Duration::from_millis(
            self.config.heartbeat_interval,
        )));
        self.waker.wake();
    }

    fn reset_election_timeout(&mut self) {
        self.election_timeout = Some(Delay::new(Duration::from_millis(thread_rng().gen_range(
            self.config.election_timeout_min,
            self.config.election_timeout_max,
        ))));
        self.waker.wake();
    }

    fn do_send_entries(&mut self, target: NodeId, target_info: Arc<N>) -> Result<()> {
        if self.role != Role::Leader {
            return Ok(());
        }
        let network = self.network.clone();
        let next_index = self.next_index.get(&target).copied().unwrap();

        let (prev_log_index, prev_log_term, entries) = match next_index {
            2..=u64::MAX => {
                let start = next_index - 1;
                let end = start + self.config.max_payload_entries as u64 + 1;
                let mut entries = self
                    .storage
                    .get_log_entries(start, end)
                    .map_err(RaftError::Storage)?;
                let prev = entries.remove(0);
                (prev.index, prev.term, entries)
            }
            1 => {
                let entries = self
                    .storage
                    .get_log_entries(
                        next_index,
                        next_index + self.config.max_payload_entries as u64,
                    )
                    .map_err(RaftError::Storage)?;
                (0, 0, entries)
            }
            _ => unreachable!(),
        };

        let append_entries = AppendEntriesRequest {
            term: self.current_term,
            leader_id: self.id,
            prev_log_index,
            prev_log_term,
            leader_commit: self.commit_index,
            entries,
        };

        debug!(
            name = self.name.as_str(),
            id = self.id,
            target = target,
            term = append_entries.term,
            leader_id = append_entries.leader_id,
            prev_log_index = append_entries.prev_log_index,
            prev_log_term = append_entries.prev_log_term,
            leader_commit = append_entries.leader_commit,
            entries = append_entries.entries.len(),
            "Core::send_append_entries",
        );

        let last_log_index = append_entries.prev_log_index + append_entries.entries.len() as u64;

        let current_term = self.current_term;
        let fut = async move {
            let resp = network
                .append_entries(target, &target_info, append_entries)
                .await?;
            Ok((target, current_term, last_log_index, resp))
        };
        assert!(!self
            .append_entries_futures
            .as_ref()
            .unwrap()
            .contains_key(&target));
        self.append_entries_futures
            .as_mut()
            .unwrap()
            .insert(target, fut.boxed());
        self.waker.wake();
        Ok(())
    }

    fn send_append_entries(&mut self) -> Result<()> {
        if self.role != Role::Leader {
            return Ok(());
        }

        for (target, target_info) in self.membership.other_members(self.id) {
            let append_entries_futures = self.append_entries_futures.as_mut().unwrap();
            if append_entries_futures.contains_key(&target) {
                self.next_append_entries_futures.insert(target, true);
                continue;
            }
            self.do_send_entries(target, target_info)?;
        }

        Ok(())
    }

    fn switch_to_follower(&mut self, term: u64) -> Result<()> {
        debug!(
            name = self.name.as_str(),
            id = self.id,
            "Become to follower",
        );
        self.role = Role::Follower;
        if self.current_term != term {
            self.current_term = term;
            self.storage
                .save_hard_state(HardState {
                    current_term: term,
                    voted_for: self.voted_for,
                })
                .map_err(RaftError::Storage)?;
        }
        self.leader = None;
        self.heartbeat_timeout = None;
        self.reset_election_timeout();
        Ok(())
    }

    fn handle_election_timeout(&mut self) -> Result<()> {
        if let Role::Candidate | Role::Follower = self.role {
            self.role = Role::Candidate;
            self.current_term += 1;
            self.voted_for = None;
            self.leader = None;
            if self.role == Role::Follower {
                debug!(
                    name = self.name.as_str(),
                    id = self.id,
                    current_term = self.current_term,
                    "Become to candidate",
                );
            }
            self.voted = std::iter::once(self.id).collect(); // vote for current node
            self.storage
                .save_hard_state(HardState {
                    current_term: self.current_term,
                    voted_for: self.voted_for,
                })
                .map_err(RaftError::Storage)?;
            self.reset_election_timeout();

            for (target, target_info) in self.membership.other_voter_members(self.id) {
                let network = self.network.clone();
                let vote_request = VoteRequest {
                    term: self.current_term,
                    candidate_id: self.id,
                    last_log_index: self.last_log_index,
                    last_log_term: self.last_log_term,
                };
                debug!(
                    name = self.name.as_str(),
                    id = self.id,
                    target = target,
                    term = self.current_term,
                    "Send vote request",
                );

                let current_term = self.current_term;
                let fut = async move {
                    let resp = network.vote(target, &target_info, vote_request).await?;
                    Ok((target, current_term, resp))
                };
                self.vote_futures.add(target, fut);
                self.waker.wake();
            }
        }

        Ok(())
    }

    fn handle_append_entries_response(
        &mut self,
        (from, term, last_log_index, resp): (u64, u64, u64, AppendEntriesResponse),
    ) -> Result<()> {
        if term != self.current_term {
            return Ok(());
        }
        if resp.term > self.current_term {
            self.switch_to_follower(resp.term)?;
        }
        if self.role != Role::Leader {
            return Ok(());
        }
        if resp.success {
            let next_index = self.next_index.get_mut(&from).unwrap();
            *next_index = last_log_index + 1;
            *self.match_index.get_mut(&from).unwrap() = last_log_index;
        } else {
            let next_index = self.next_index.get_mut(&from).unwrap();
            if *next_index > 1 {
                *next_index -= 1;
            }
        }
        Ok(())
    }

    fn handle_vote_response(&mut self, (from, term, resp): (u64, u64, VoteResponse)) -> Result<()> {
        if term != self.current_term || self.role != Role::Candidate {
            return Ok(());
        }
        if resp.term > self.current_term {
            self.switch_to_follower(resp.term)?;
            return Ok(());
        }
        self.voted.insert(from);
        if self.voted.len() > self.membership.members.len() / 2 {
            self.become_to_leader()?;
        }
        Ok(())
    }

    fn become_to_leader(&mut self) -> Result<()> {
        debug!(
            name = self.name.as_str(),
            id = self.id,
            voted = self.voted.len(),
            members = self.membership.members.len(),
            "Become to leader",
        );
        self.role = Role::Leader;
        self.leader = Some(self.id);
        self.next_index = self
            .membership
            .other_members(self.id)
            .map(|(id, _)| (id, self.last_log_index + 1))
            .collect();
        self.election_timeout = None;
        self.match_index = self
            .membership
            .other_members(self.id)
            .map(|(id, _)| (id, 0))
            .collect();
        self.append_entries_futures = Some(Default::default());
        self.next_append_entries_futures = Default::default();
        self.reset_heartbeat_timeout();
        self.send_append_entries()?;

        Ok(())
    }

    fn handle_initialize(
        &mut self,
        members: Vec<(NodeId, N)>,
        reply: oneshot::Sender<Result<()>>,
    ) -> Result<()> {
        if self.current_term > 0 || self.last_log_index > 0 {
            return Err(RaftError::AlreadyInitialized);
        }

        debug!(
            term = self.current_term,
            index = self.last_log_index,
            members = members.len(),
            "Core::initialize",
        );

        self.become_to_leader()?;
        self.append_entry(
            Entry {
                term: self.current_term,
                index: self.last_log_index + 1,
                detail: EntryDetail::ChangeMemberShip(MemberShipConfig {
                    members: members
                        .into_iter()
                        .map(|(id, info)| (id, Arc::new(info)))
                        .collect(),
                    non_voters: Default::default(),
                }),
            },
            reply,
        )
    }

    fn handle_vote(&mut self, req: VoteRequest) -> Result<VoteResponse> {
        if self.role == Role::NonVoter {
            return Ok(VoteResponse {
                term: self.current_term,
                vote_granted: false,
            });
        }

        debug!(
            name = self.name.as_str(),
            id = self.id,
            current_term = self.current_term,
            term = req.term,
            candidate_id = req.candidate_id,
            last_log_index = req.last_log_index,
            last_log_term = req.last_log_term,
            "Core::handle_vote",
        );

        if req.term < self.current_term {
            // Reply false if term < currentTerm
            return Ok(VoteResponse {
                term: self.current_term,
                vote_granted: false,
            });
        }

        if req.term > self.current_term {
            self.switch_to_follower(req.term)?;
        }

        if (self.voted_for.is_none() || self.voted_for == Some(req.candidate_id))
            && req.last_log_term >= self.last_log_term
            && req.last_log_index >= self.last_log_index
        {
            debug!(
                name = self.name.as_str(),
                id = self.id,
                candidate_id = req.candidate_id,
                "Core::vote_for",
            );
            self.voted_for = Some(req.candidate_id);
            self.reset_election_timeout();
            return Ok(VoteResponse {
                term: self.current_term,
                vote_granted: true,
            });
        }

        Ok(VoteResponse {
            term: self.current_term,
            vote_granted: false,
        })
    }

    fn handle_append_entries(
        &mut self,
        req: AppendEntriesRequest<N, D>,
    ) -> Result<AppendEntriesResponse> {
        debug!(
            name = self.name.as_str(),
            id = self.id,
            term = req.term,
            current_term = self.current_term,
            leader_id = req.leader_id,
            prev_log_index = req.prev_log_index,
            prev_log_term = req.prev_log_term,
            leader_commit = req.leader_commit,
            entries = req.entries.len(),
            "Core::handle_append_entries",
        );

        if req.term < self.current_term {
            // Reply false if term < currentTerm
            return Ok(AppendEntriesResponse {
                term: self.current_term,
                success: false,
            });
        }

        if (self.role != Role::Follower && self.role != Role::NonVoter)
            || (self.role == Role::NonVoter && self.last_log_index == 0)
        {
            self.switch_to_follower(req.term)?;
        }

        if self.leader != Some(req.leader_id) {
            debug!(
                name = self.name.as_str(),
                id = self.id,
                leader_id = req.leader_id,
                "Core::follow to leader",
            );
            self.leader = Some(req.leader_id);
        }

        self.reset_election_timeout();

        if req.leader_commit > self.commit_index {
            self.commit_index = self.last_log_index.min(req.leader_commit);
        }

        if req.entries.is_empty() {
            // Is heartbeat message
            return Ok(AppendEntriesResponse {
                term: self.current_term,
                success: true,
            });
        }

        if self.last_log_index > 0 || self.last_log_term > 0 {
            match self
                .storage
                .get_log_entries(self.last_log_index, self.last_log_index + 1)
                .map_err(RaftError::Storage)?
                .into_iter()
                .next()
            {
                Some(log) if log.term != self.last_log_term => {
                    // If an entry conflicts with a new one(same index buf different terms), delete the
                    // existing entry and all that follow it
                    self.storage
                        .delete_logs_from(log.index, None)
                        .map_err(RaftError::Storage)?;
                }
                Some(_) => {}
                None => {
                    // Reply false if log doesn't contain an entry at prevLogIndex whose term matches prevLogTerm
                    return Ok(AppendEntriesResponse {
                        term: self.current_term,
                        success: false,
                    });
                }
            }
        }

        // Append any new entries not already in the log
        self.storage
            .append_entries_to_log(&req.entries)
            .map_err(RaftError::Storage)?;
        self.last_log_index = req.entries.last().unwrap().index;
        self.last_log_term = req.entries.last().unwrap().term;

        Ok(AppendEntriesResponse {
            term: self.current_term,
            success: true,
        })
    }

    fn append_entry(
        &mut self,
        entry: Entry<N, D>,
        reply: oneshot::Sender<Result<()>>,
    ) -> Result<()> {
        if self.role != Role::Leader {
            reply
                .send(Err(RaftError::ForwardToLeader(self.leader)))
                .ok();
            return Ok(());
        }

        debug!(term = entry.term, index = entry.index, "Core::append_entry",);
        let res = self
            .pending_write
            .insert(entry.index, std::iter::once(reply).collect());
        assert!(res.is_none());
        self.storage
            .append_entries_to_log(&[entry])
            .map_err(RaftError::Storage)?;
        self.last_log_index += 1;
        self.send_append_entries()?;

        Ok(())
    }

    fn handle_client_read(&mut self, reply: oneshot::Sender<Result<()>>) -> Result<()> {
        self.append_entry(
            Entry {
                term: self.current_term,
                index: self.last_log_index + 1,
                detail: EntryDetail::Blank,
            },
            reply,
        )
    }

    fn handle_client_write(&mut self, action: D, reply: oneshot::Sender<Result<()>>) -> Result<()> {
        self.append_entry(
            Entry {
                term: self.current_term,
                index: self.last_log_index + 1,
                detail: EntryDetail::Normal(action),
            },
            reply,
        )
    }

    fn handle_metrics(&self) -> Result<Metrics> {
        Ok(Metrics {
            id: self.id,
            role: self.role,
            current_term: self.current_term,
            last_log_index: self.last_log_index,
            last_applied: self.storage.last_applied().unwrap(),
            has_leader: self.leader.is_some(),
            current_leader: self.leader.unwrap_or_default(),
        })
    }

    fn handle_message(&mut self, msg: Message<N, D>) -> Result<bool> {
        match msg {
            Message::Initialize { members, reply } => {
                self.handle_initialize(members, reply)?;
                Ok(true)
            }
            Message::Vote { req, reply } => {
                reply.send(self.handle_vote(req)).ok();
                Ok(true)
            }
            Message::AppendEntries { req, reply } => {
                reply.send(self.handle_append_entries(req)).ok();
                Ok(true)
            }
            Message::ClientRead { reply } => {
                self.handle_client_read(reply)?;
                Ok(true)
            }
            Message::ClientWrite {
                action: actions,
                reply,
            } => {
                self.handle_client_write(actions, reply)?;
                Ok(true)
            }
            Message::AddNode { .. } => {
                todo!()
            }
            Message::RemoveNode { .. } => {
                todo!()
            }
            Message::Metrics { reply } => {
                reply.send(self.handle_metrics()).ok();
                Ok(true)
            }
            Message::Shutdown => Ok(false),
        }
    }

    fn reply_client_write(&mut self, before: u64, res: impl Fn() -> Result<()>) {
        let items: Vec<_> = self
            .pending_write
            .range(0..before)
            .map(|(index, _)| *index)
            .collect();
        for index in items {
            if let Some(senders) = self.pending_write.remove(&index) {
                for tx in senders {
                    tx.send(res()).ok();
                }
            }
        }
    }

    fn update_commit_index(&mut self) {
        if self.membership.members.len() == 1 && self.last_log_index > self.commit_index {
            self.commit_index = self.last_log_index;
            debug!(
                name = self.name.as_str(),
                id = self.id,
                commit_index = self.commit_index,
                "Core::commit_index",
            );
            return;
        }

        let mut match_index: SmallVec<[u64; 16]> = self.match_index.values().copied().collect();
        match_index.sort_by_key(|&b| Reverse(b));
        for idx in &match_index {
            if *idx > self.commit_index
                && match_index.iter().filter(|n| **n >= *idx).count()
                    >= self.membership.members.len() / 2
            {
                self.commit_index = *idx;
                debug!(
                    name = self.name.as_str(),
                    id = self.id,
                    commit_index = self.commit_index,
                    "Core::commit_index",
                );
                break;
            }
        }
    }

    fn apply_to_state_machine_if_needed(&mut self) -> Result<Option<u64>> {
        let last_applied = self.storage.last_applied().map_err(RaftError::Storage)?;
        if self.commit_index > last_applied {
            let entries = self
                .storage
                .get_log_entries(last_applied + 1, self.commit_index + 1)
                .map_err(RaftError::Storage)?;

            for entry in 

            if !entries.is_empty() {
                debug!(
                    name = self.name.as_str(),
                    id = self.id,
                    from = last_applied + 1,
                    to = entries.last().unwrap().index + 1,
                    entries = entries.len(),
                    "Core::apply_to_state_machine",
                );
                self.storage
                    .apply_entries_to_state_machine(&entries)
                    .map_err(RaftError::Storage)?;
            }
            Ok(entries.last().map(|entry| entry.index + 1))
        } else {
            Ok(None)
        }
    }
}

impl<N, D> Future for Core<N, D>
where
    N: Send + Sync + Unpin + 'static,
    D: Send + Unpin + 'static,
{
    type Output = Result<()>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = &mut *self;
        this.waker.register(cx.waker());

        if let Some(heartbeat_timeout) = &mut this.heartbeat_timeout {
            if let Poll::Ready(_) = heartbeat_timeout.poll_unpin(cx) {
                debug!(
                    name = this.name.as_str(),
                    id = this.id,
                    "Core::heartbeat_timeout"
                );
                if this.role == Role::Leader {
                    this.reset_heartbeat_timeout();
                    if let Err(err) = this.send_append_entries() {
                        return Poll::Ready(Err(err));
                    }
                } else {
                    this.heartbeat_timeout = None;
                }
            }
        }

        if let Some(election_timeout) = &mut this.election_timeout {
            if let Poll::Ready(_) = election_timeout.poll_unpin(cx) {
                debug!(
                    name = this.name.as_str(),
                    id = this.id,
                    "Core::election_timeout",
                );
                if let Role::Follower | Role::Candidate = this.role {
                    if let Err(err) = this.handle_election_timeout() {
                        return Poll::Ready(Err(err));
                    }
                } else {
                    this.election_timeout = None;
                }
            }
        }

        {
            let mut append_entries_futures = this.append_entries_futures.take().unwrap();
            let mut remove_list = SmallVec::<[u64; 8]>::new();
            for (id, fut) in &mut append_entries_futures {
                let remove = match fut.poll_unpin(cx) {
                    Poll::Ready(Ok(resp)) => {
                        if let Err(err) = this.handle_append_entries_response(resp) {
                            return Poll::Ready(Err(err));
                        }
                        true
                    }
                    Poll::Ready(Err(_)) => true,
                    Poll::Pending => false,
                };
                if remove {
                    remove_list.push(*id);
                }
            }
            for id in &remove_list {
                append_entries_futures.remove(id);
            }
            this.append_entries_futures = Some(append_entries_futures);

            for id in remove_list {
                if this
                    .next_append_entries_futures
                    .get(&id)
                    .copied()
                    .unwrap_or_default()
                {
                    if let Some(member_info) = this.membership.member_info(id) {
                        if let Err(err) = this.do_send_entries(id, member_info) {
                            return Poll::Ready(Err(err));
                        }
                        this.next_append_entries_futures.insert(id, false);
                    }
                }
            }
        }

        loop {
            match this.vote_futures.poll_next_unpin(cx) {
                Poll::Ready(Some(Ok(resp))) => {
                    if let Err(err) = this.handle_vote_response(resp) {
                        return Poll::Ready(Err(err));
                    }
                }
                Poll::Ready(Some(Err(_err))) => {
                    // error!(
                    //     "Failed to send Vote request",
                    //     id = this.id,
                    //     error = err.to_string()
                    // );
                }
                _ => break,
            }
        }

        loop {
            match this.rx.poll_next_unpin(cx) {
                Poll::Ready(Some(msg)) => match this.handle_message(msg) {
                    Ok(res) => {
                        if !res {
                            return Poll::Ready(Ok(()));
                        }
                    }
                    Err(err) => return Poll::Ready(Err(err)),
                },
                Poll::Ready(None) => {
                    // Raft is shutdown
                    return Poll::Ready(Ok(()));
                }
                Poll::Pending => break,
            }
        }

        if this.role == Role::Leader {
            this.update_commit_index();
            match self.apply_to_state_machine_if_needed() {
                Ok(Some(last_id)) => self.reply_client_write(last_id + 1, || Ok(())),
                Ok(None) => {}
                Err(err) => return Poll::Ready(Err(err)),
            }
        } else {
            let leader = this.leader;
            this.reply_client_write(u64::MAX, || Err(RaftError::ForwardToLeader(leader)));
            if let Err(err) = self.apply_to_state_machine_if_needed() {
                return Poll::Ready(Err(err));
            }
        }

        Poll::Pending
    }
}
