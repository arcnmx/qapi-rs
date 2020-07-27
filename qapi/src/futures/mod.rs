#[cfg(feature = "qapi-qmp")]
use qapi_qmp::{QmpMessage, QmpMessageAny, QapiCapabilities, QMPCapability};

use qapi_spec::Response;
use crate::{Any, Execute, ExecuteResult, Command};

use std::collections::BTreeMap;
use std::convert::TryInto;
use std::marker::Unpin;
use std::sync::{Arc, Mutex as StdMutex, atomic::{AtomicUsize, AtomicBool, Ordering}};
use std::task::{Context, Poll};
use std::pin::Pin;
use std::io;
use futures::channel::oneshot;
use futures::task::AtomicWaker;
use futures::lock::Mutex;
use futures::{Future, FutureExt, Sink, SinkExt, Stream};
use serde::Deserialize;
use log::{trace, info, warn};

#[cfg(any(feature = "futures_codec", feature = "tokio-util"))]
mod codec;

#[cfg(feature = "tokio")]
mod tokio;
#[cfg(feature = "tokio")]
pub use self::tokio::*;

#[cfg(feature = "tower-service")]
mod tower;

pub struct QapiStream<R, W> {
    service: QapiService<W>,
    events: QapiEvents<R>,
}

impl<R, W> QapiStream<R, W> {
    pub fn with_parts(service: QapiService<W>, events: QapiEvents<R>) -> Self {
        Self {
            service,
            events,
        }
    }

    pub fn into_parts(self) -> (QapiService<W>, QapiEvents<R>) {
        (self.service, self.events)
    }

    #[cfg(feature = "async-tokio-spawn")]
    pub fn spawn_tokio(self) -> (QapiService<W>, ::tokio::task::JoinHandle<()>) where QapiEvents<R>: Future<Output=io::Result<()>> + Send + 'static {
        let handle = self.events.spawn_tokio();
        (self.service, handle)
    }

    pub fn execute<'a, C: Command + 'a>(&'a mut self, command: C) -> impl Future<Output=ExecuteResult<C>> + 'a where
        QapiEvents<R>: Future<Output=io::Result<()>> + Unpin,
        W: Sink<Execute<C, u32>, Error=io::Error> + Unpin
    {
        let execute = self.service.execute(command).fuse();

        async move {
            futures::pin_mut!(execute);

            futures::select_biased! {
                res = execute => res,
                res = (&mut self.events).fuse() => {
                    res?;
                    Err(io::Error::new(io::ErrorKind::UnexpectedEof, "unexpected EOF when executing command").into())
                },
            }
        }
    }
}

#[cfg(feature = "qapi-qmp")]
pub struct QmpStreamNegotiation<S, W> {
    pub stream: QapiStream<S, W>,
    pub capabilities: QapiCapabilities,
}

#[cfg(feature = "qapi-qmp")]
impl<S, W> QmpStreamNegotiation<S, W> where
    QapiEvents<S>: Future<Output=io::Result<()>> + Unpin,
    W: Sink<Execute<qapi_qmp::qmp_capabilities, u32>, Error=io::Error> + Unpin,
{
    pub async fn negotiate_caps<C>(mut self, caps: C) -> io::Result<QapiStream<S, W>> where
        C: IntoIterator<Item=QMPCapability>,
    {
        let _ = self.stream.execute(qapi_qmp::qmp_capabilities {
            enable: Some(caps.into_iter().collect()),
        }).await?;

        Ok(self.stream)
    }

    pub async fn negotiate(self) -> io::Result<QapiStream<S, W>> {
        self.negotiate_caps(std::iter::empty()).await
    }
}

type QapiCommandMap = BTreeMap<u32, oneshot::Sender<Result<Any, qapi_spec::Error>>>;

pub struct QapiService<W> {
    shared: Arc<QapiShared>,
    write: Arc<Mutex<W>>,
    id_counter: AtomicUsize,
}

impl<W> QapiService<W> {
    fn new(write: W, shared: Arc<QapiShared>) -> Self {
        QapiService {
            shared,
            write: Mutex::new(write).into(),
            id_counter: AtomicUsize::new(0),
        }
    }

    fn next_oob_id(&self) -> u32 {
        self.id_counter.fetch_add(1, Ordering::Relaxed) as _
    }

    fn command_id(&self) -> Option<u32> {
        if self.shared.supports_oob {
            Some(self.next_oob_id())
        } else {
            None
        }
    }

    fn command_response<C: Command>(receiver: oneshot::Receiver<Result<Any, qapi_spec::Error>>) -> impl Future<Output=ExecuteResult<C>> {
        receiver.map(|res| match res {
            Ok(Ok(res)) => C::Ok::deserialize(&res)
                .map_err(io::Error::from).map_err(From::from),
            Ok(Err(e)) => Err(e.into()),
            Err(_cancelled) => Err(io::Error::new(io::ErrorKind::UnexpectedEof, "QAPI stream disconnected").into()),
        })
    }

    pub fn execute<C: Command>(&self, command: C) -> impl Future<Output=ExecuteResult<C>> where
        W: Sink<Execute<C, u32>, Error=io::Error> + Unpin
    {
        let id = self.command_id();
        let sink = self.write.clone();
        let shared = self.shared.clone();
        let command = Execute::new(command, id);

        async move {
            let mut sink = sink.lock().await;
            let receiver = shared.command_insert(id.unwrap_or_default());

            sink.send(command).await?;
            if id.is_some() {
                // retain write lock only if id/oob execution isn't supported
                drop(sink)
            }

            Self::command_response::<C>(receiver).await
        }
    }

    /*pub async fn execute_oob<C: Command>(&self, command: C) -> io::Result<ExecuteResult<C>> {
        /* TODO: should we assert C::ALLOW_OOB here and/or at the type level?
         * If oob isn't supported should we fall back to serial execution or error?
         */
        self.execute_(command, true).await
    }*/

    #[cfg(feature = "qapi-qga")]
    pub fn guest_sync(&self, sync_value: isize) -> impl Future<Output=Result<(), crate::ExecuteError>> where
        W: Sink<Execute<qapi_qga::guest_sync, u32>, Error=io::Error> + Unpin
    {
        self.execute(qapi_qga::guest_sync {
            id: sync_value,
        }).map(move |res| res.and_then(|res| if res == sync_value {
            Ok(())
        } else {
            Err(io::Error::new(io::ErrorKind::InvalidData, "QGA sync failed").into())
        }))
    }

    fn stop(&self) {
        let mut commands = self.shared.commands.lock().unwrap();
        if self.shared.abandoned.load(Ordering::Relaxed) {
            self.shared.stop();
        }
        commands.abandoned = true;
    }
}

impl<W> Drop for QapiService<W> {
    fn drop(&mut self) {
        self.stop();
    }
}

#[derive(Default)]
struct QapiSharedCommands {
    pending: QapiCommandMap,
    abandoned: bool,
}

struct QapiShared {
    commands: StdMutex<QapiSharedCommands>,
    stop_waker: AtomicWaker,
    stop: AtomicBool,
    abandoned: AtomicBool,
    supports_oob: bool,
}

impl QapiShared {
    fn new(supports_oob: bool) -> Self {
        Self {
            commands: Default::default(),
            stop_waker: Default::default(),
            stop: Default::default(),
            abandoned: Default::default(),
            supports_oob,
        }
    }

    fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
        self.stop_waker.wake();
    }

    fn is_stopped(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }

    fn poll_next<T, P: FnOnce(&mut Context) -> Poll<Option<T>>>(&self, cx: &mut Context, poll: P) -> Poll<Option<T>> {
        if self.is_stopped() {
            return Poll::Ready(None)
        }

        // attempt to complete the future
        match poll(cx) {
            Poll::Ready(res) => {
                if res.is_none() {
                    self.stop.store(true, Ordering::Relaxed);
                }
                Poll::Ready(res)
            },
            Poll::Pending => {
                self.stop_waker.register(cx.waker());
                if self.is_stopped() {
                    Poll::Ready(None)
                } else {
                    Poll::Pending
                }
            },
        }
    }

    fn command_remove(&self, id: u32) -> Option<oneshot::Sender<Result<Any, qapi_spec::Error>>> {
        let mut commands = self.commands.lock().unwrap();
        commands.pending.remove(&id)
    }

    fn command_insert(&self, id: u32) -> oneshot::Receiver<Result<Any, qapi_spec::Error>> {
        let (sender, receiver) = oneshot::channel();
        let mut commands = self.commands.lock().unwrap();
        if !commands.abandoned {
            // otherwise sender is dropped immediately
            if let Some(_prev) = commands.pending.insert(id, sender) {
                panic!("QAPI duplicate command id {:?}, this should not happen", id);
            }
        }
        receiver
    }
}

#[must_use]
pub struct QapiEvents<S> {
    stream: S,
    shared: Arc<QapiShared>,
}

impl<S> QapiEvents<S> {
    pub async fn into_future(self) -> () where
        Self: Future<Output=io::Result<()>>,
    {
        {
            let commands = self.shared.commands.lock().unwrap();
            if commands.abandoned {
                info!("QAPI service abandoned before spawning");
                drop(commands);
                drop(self);
                return
            }
            self.shared.abandoned.store(true, Ordering::Relaxed);
        }

        match self.await {
            Ok(()) => (),
            Err(e) =>
                warn!("QAPI stream closed with error {:?}", e),
        }
    }

    pub fn spawn<SP: futures::task::Spawn>(self, spawn: SP) -> Result<(), futures::task::SpawnError> where
        Self: Future<Output=io::Result<()>> + Send + 'static,
        S: 'static
    {
        use futures::task::SpawnExt;

        spawn.spawn(self.into_future())
    }

    #[cfg(feature = "async-tokio-spawn")]
    pub fn spawn_tokio(self) -> ::tokio::task::JoinHandle<()> where
        Self: Future<Output=io::Result<()>> + Send + 'static,
        S: 'static
    {
        ::tokio::spawn(self.into_future())
    }
}

impl<S> Drop for QapiEvents<S> {
    fn drop(&mut self) {
        let mut commands = self.shared.commands.lock().unwrap();
        commands.pending.clear();
        commands.abandoned = true;
    }
}

fn response_id<T>(res: &Response<T>, supports_oob: bool) -> io::Result<u32> {
    match (res.id().and_then(|id| id.as_u64()), supports_oob) {
        (Some(id), true) =>
            id.try_into().map_err(|e|
                io::Error::new(io::ErrorKind::InvalidData, e)
            ),
        (None, false) =>
            Ok(Default::default()),
        (None, true) =>
            Err(io::Error::new(io::ErrorKind::InvalidData, format!("QAPI expected response with numeric ID, got {:?}", res.id()))),
        (Some(..), false) =>
            Err(io::Error::new(io::ErrorKind::InvalidData, format!("QAPI expected response without ID, got {:?}", res.id()))),
    }
}

fn handle_response(shared: &QapiShared, res: Response<Any>) -> io::Result<()> {
    let id = response_id(&res, shared.supports_oob)?;

    if let Some(sender) = shared.command_remove(id) {
        sender.send(res.result()).map_err(|_e|
            io::Error::new(io::ErrorKind::InvalidData, format!("failed to send response for ID {:?}", id))
        )
    } else {
        Err(io::Error::new(io::ErrorKind::InvalidData, format!("unknown QAPI response with ID {:?}", res.id())))
    }
}

impl<M, S> Future for QapiEvents<S> where
    S: Stream<Item=io::Result<M>>,
    M: TryInto<Response<Any>>,
{
    type Output = io::Result<()>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = unsafe { self.get_unchecked_mut() };
        let stream = unsafe { Pin::new_unchecked(&mut this.stream) };
        let shared = &this.shared;

        shared.poll_next(cx, |cx| Poll::Ready(Some(match futures::ready!(stream.poll_next(cx)) {
            None => return Poll::Ready(None),
            Some(Err(e)) => Err(e),
            Some(Ok(res)) => match res.try_into() {
                Ok(res) => match handle_response(shared, res) {
                    Err(e) => Err(e),
                    Ok(()) => {
                        cx.waker().wake_by_ref(); // TODO: I've seen this not work with tokio?
                        return Poll::Pending
                    },
                },
                Err(..) => {
                    trace!("Ignoring QAPI event");
                    cx.waker().wake_by_ref(); // TODO: I've seen this not work with tokio?
                    return Poll::Pending
                },
            },
        }))).map(|res| res.unwrap_or(Ok(())))
    }
}

#[cfg(feature = "qapi-qmp")]
impl<S: Stream<Item=io::Result<QmpMessageAny>>> Stream for QapiEvents<S> {
    type Item = io::Result<qapi_qmp::Event>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = unsafe { self.get_unchecked_mut() };
        let stream = unsafe { Pin::new_unchecked(&mut this.stream) };
        let shared = &this.shared;

        shared.poll_next(cx, |cx| Poll::Ready(match futures::ready!(stream.poll_next(cx)) {
            None => None, // eof
            Some(Err(e)) => Some(Err(e)),
            Some(Ok(QmpMessage::Event(e))) => Some(Ok(e)),
            Some(Ok(QmpMessage::Response(res))) => match handle_response(shared, res) {
                Err(e) => Some(Err(e)),
                Ok(()) => {
                    cx.waker().wake_by_ref(); // TODO: I've seen this not work with tokio?
                    return Poll::Pending
                },
            },
        }))
    }
}
