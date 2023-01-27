use {
    super::QapiService,
    crate::{Command, Execute, ExecuteError},
    futures::{Future, FutureExt, Sink},
    std::{
        io,
        pin::Pin,
        task::{Context, Poll},
    },
    tower_service::Service,
};

// this really doesn't work well for lifetime reasons?

impl<W: 'static, C: Command + 'static> Service<C> for QapiService<W>
where
    W: Sink<Execute<C, u32>, Error = io::Error> + Unpin + Send,
{
    type Error = ExecuteError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + 'static>>;
    type Response = C::Ok;

    fn poll_ready(&mut self, _: &mut Context) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: C) -> Self::Future {
        self.execute(req).boxed()
    }
}
