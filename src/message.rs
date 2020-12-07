use futures::channel::oneshot;

use crate::{
    AppendEntriesRequest, AppendEntriesResponse, ClientError, RaftError, Result, VoteRequest,
    VoteResponse,
};

pub enum Message<D> {
    Initialize {
        reply: oneshot::Sender<Result<()>>,
    },
    Vote {
        req: VoteRequest,
        reply: oneshot::Sender<Result<VoteResponse>>,
    },
    AppendEntries {
        req: AppendEntriesRequest<D>,
        reply: oneshot::Sender<Result<AppendEntriesResponse>>,
    },
    ClientWrite {
        actions: Vec<WriteAction>,
        reply: oneshot::Sender<Result<(), ClientError>>,
    },
    ClientRead {
        reply: oneshot::Sender<Result<(), ClientError>>,
    },
    Metrics {
        reply: oneshot::Sender<Result<RaftMetrics>>,
    },
    Shutdown,
}
