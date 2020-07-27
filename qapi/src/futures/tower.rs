use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};
use futures::{Sink, Future, FutureExt};
use tower_service::Service;
use crate::{Command, Execute, ExecuteError};
use super::QapiService;

// this really doesn't work well for lifetime reasons?

impl<W: 'static, C: Command + 'static> Service<C> for QapiService<W> where
    W: Sink<Execute<C, u32>, Error=io::Error> + Unpin + Send,
{
    type Response = C::Ok;
    type Error = ExecuteError;
    type Future = Pin<Box<dyn Future<Output=Result<Self::Response, Self::Error>> + 'static>>;

    fn poll_ready(&mut self, _: &mut Context) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: C) -> Self::Future {
        self.execute(req).boxed()
    }
}
