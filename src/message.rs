use futures::channel::oneshot;

use crate::{
    AppendEntriesRequest, AppendEntriesResponse, ClientError, Metrics, RaftError, Result,
    VoteRequest, VoteResponse,
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
        actions: D,
        reply: oneshot::Sender<Result<(), ClientError>>,
    },
    ClientRead {
        reply: oneshot::Sender<Result<(), ClientError>>,
    },
    Metrics {
        reply: oneshot::Sender<Result<Metrics>>,
    },
    Shutdown,
}
