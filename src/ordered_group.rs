use fnv::FnvHashMap;
use futures::future::BoxFuture;
use futures::task::{Context, Poll};
use futures::{Future, FutureExt, Stream, StreamExt};
use tokio::macros::support::Pin;

use crate::NodeId;

type FuturesOrdered<T> = futures::stream::FuturesOrdered<BoxFuture<'static, T>>;

pub struct OrderedGroup<T>(FnvHashMap<NodeId, FuturesOrdered<T>>);

impl<T> Default for OrderedGroup<T> {
    fn default() -> Self {
        Self(Default::default())
    }
}

impl<T> OrderedGroup<T> {
    pub fn add<F>(&mut self, id: NodeId, fut: F)
    where
        F: Future<Output = T> + Send + 'static,
    {
        match self.0.get_mut(&id) {
            Some(futs) => futs.push(fut.boxed()),
            None => {
                let mut futs = FuturesOrdered::default();
                futs.push(fut.boxed());
                self.0.insert(id, futs);
            }
        }
    }
}

impl<T> Stream for OrderedGroup<T> {
    type Item = T;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = &mut *self;
        for futs in this.0.values_mut() {
            match futs.poll_next_unpin(cx) {
                Poll::Ready(Some(res)) => return Poll::Ready(Some(res)),
                Poll::Pending | Poll::Ready(None) => {}
            }
        }
        Poll::Pending
    }
}
