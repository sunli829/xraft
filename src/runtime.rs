#[cfg(feature = "tokio")]
mod tokio_runtime {
    use futures::pin_mut;
    use futures::{Future, FutureExt};
    use std::pin::Pin;
    use std::task::{Context, Poll};

    pub struct Delay(tokio::time::Sleep);

    impl Future for Delay {
        type Output = ();

        fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            self.0.poll_unpin(cx)
        }
    }

    pub struct JoinHandle<T>(tokio::task::JoinHandle<T>);

    impl<T> Future for JoinHandle<T> {
        type Output = T;

        fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            match self.0.poll_unpin(cx) {
                Poll::Ready(res) => Poll::Ready(res.unwrap()),
                Poll::Pending => Poll::Pending,
            }
        }
    }

    pub fn spawn<T>(task: T) -> JoinHandle<T::Output>
    where
        T: Future + Send + 'static,
        T::Output: Send + 'static,
    {
        JoinHandle(tokio::spawn(task))
    }
}

#[cfg(feature = "tokio")]
pub use tokio_runtime::{spawn, Delay, JoinHandle};
