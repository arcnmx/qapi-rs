#![allow(unused_imports)]
#![doc(html_root_url = "http://docs.rs/tokio-qapi/0.4.0")]

use qapi_spec as spec;

#[cfg(feature = "qapi-qmp")]
pub use qapi_qmp as qmp;

#[cfg(feature = "qapi-qga")]
pub use qapi_qga as qga;

pub use qapi_spec::{Any, Dictionary, Empty, Execute, ExecuteOob, Command, CommandResult, Event, Error, ErrorClass, Timestamp};

use std::collections::BTreeMap;
use std::convert::TryInto;
use std::sync::{Arc, atomic::{AtomicUsize, AtomicBool, Ordering}};
use std::task::Context;
use std::mem::replace;
use std::{io, str, usize};
use result::OptionResultExt;
use futures::channel::oneshot;
use futures::task::AtomicWaker;
//use futures::io::{AsyncRead, AsyncWrite, AsyncReadExt, AsyncWriteExt, ReadHalf, WriteHalf};
use tokio::io::{AsyncRead, AsyncWrite, AsyncReadExt, AsyncWriteExt, ReadHalf, WriteHalf, split};
use tokio_util::codec::{Framed, FramedParts, Encoder, Decoder};
use futures::{TryFutureExt, FutureExt, Sink, SinkExt, Stream, StreamExt, future, try_join};
use futures::stream::{FusedStream, unfold};
use std::pin::Pin;
use std::task::Poll;
use std::future::Future;
use std::marker::Unpin;
use std::sync::Mutex as StdMutex;
//use futures::io::{AsyncRead as _AsyncRead, AsyncWrite as _AsyncWrite, AsyncReadExt, ReadHalf, WriteHalf};
use futures::lock::Mutex;
//use futures::sync::BiLock;
//use futures::task::{self, Task};
use bytes::BytesMut;
use log::{trace, debug};

mod codec;
use codec::JsonLinesCodec;

pub struct QapiStream<R, W> {
    service: QapiService<W>,
    events: QapiEvents<R>,
}

impl<R, W> QapiStream<R, W> {
    pub fn spawn(self) -> QapiService<W> where QapiEvents<R>: Future<Output=io::Result<()>> + Send + 'static {
        self.events.spawn();
        self.service
    }

    pub fn into_parts(self) -> (QapiService<W>, QapiEvents<R>) {
        (self.service, self.events)
    }
}

pub struct QgaStream<S> {
    stream: Framed<S, codec::JsonLinesCodec<spec::Response<Any>>>
}

impl<S> QgaStream<S> {
    pub fn new(stream: S) -> Self {
        Self {
            stream: Framed::from_parts(FramedParts::new::<()>(stream, codec::JsonLinesCodec::new())),
        }
    }

    pub fn pair<W>(self, write: W) -> QapiStream<Self, W> {
        let shared = Arc::new(QapiShared::new(false));
        let events = QapiEvents {
            stream: self,
            shared: shared.clone(),
        };
        let service = QapiService::new(write, shared);
        QapiStream {
            service,
            events,
        }
    }
}

impl<RW: AsyncRead + AsyncWrite> QgaStream<ReadHalf<RW>> {
    pub fn open(stream: RW) -> QapiStream<Self, QgaStream<WriteHalf<RW>>> {
        let (r, w) = split(stream);
        let r = Self::new(r);
        let w = QgaStream::new(w);

        r.pair(w)
    }
}

impl<S> QgaStream<S> {
    fn stream(self: Pin<&mut Self>) -> Pin<&mut Framed<S, codec::JsonLinesCodec<spec::Response<Any>>>> {
        unsafe {
            self.map_unchecked_mut(|this| &mut this.stream)
        }
    }
}

impl<S: AsyncRead> Stream for QgaStream<S> {
    type Item = io::Result<spec::Response<Any>>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.stream().poll_next(cx)
    }
}

#[cfg(feature = "qapi-qga")]
impl<S: AsyncWrite, C: qga::QgaCommand, I: serde::Serialize> Sink<Execute<C, I>> for QgaStream<S> {
    type Error = io::Error;

    fn start_send(self: Pin<&mut Self>, item: Execute<C, I>) -> Result<(), Self::Error> {
        self.stream().start_send(item)
    }

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Sink::<Execute<C, I>>::poll_ready(self.stream(), cx)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Sink::<Execute<C, I>>::poll_flush(self.stream(), cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Sink::<Execute<C, I>>::poll_close(self.stream(), cx)
    }
}

#[cfg(feature = "qapi-qmp")]
pub struct QmpStream<S> {
    stream: Framed<S, codec::JsonLinesCodec<qmp::QmpMessageAny>>,
}

#[cfg(feature = "qapi-qmp")]
impl<S> QmpStream<S> {
    fn stream(self: Pin<&mut Self>) -> Pin<&mut Framed<S, codec::JsonLinesCodec<qmp::QmpMessageAny>>> {
        unsafe {
            self.map_unchecked_mut(|this| &mut this.stream)
        }
    }
}

#[cfg(feature = "qapi-qmp")]
impl<S: AsyncRead> Stream for QmpStream<S> {
    type Item = io::Result<qmp::QmpMessageAny>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.stream().poll_next(cx)
    }
}

#[cfg(feature = "qapi-qmp")]
impl<S: AsyncWrite, C: qmp::QmpCommand, I: serde::Serialize> Sink<Execute<C, I>> for QmpStream<S> {
    type Error = io::Error;

    fn start_send(self: Pin<&mut Self>, item: Execute<C, I>) -> Result<(), Self::Error> {
        self.stream().start_send(item)
    }

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Sink::<Execute<C, I>>::poll_ready(self.stream(), cx)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Sink::<Execute<C, I>>::poll_flush(self.stream(), cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Sink::<Execute<C, I>>::poll_close(self.stream(), cx)
    }
}

#[cfg(feature = "qapi-qmp")]
impl<S> QmpStream<S> {
    pub fn new(stream: S) -> Self {
        Self {
            stream: Framed::from_parts(FramedParts::new::<()>(stream, codec::JsonLinesCodec::<qmp::QmpMessageAny>::new())),
        }
    }

    pub async fn open_split<W>(read: S, write: W) -> io::Result<QmpStreamNegotiation<Self, QmpStream<W>>> where
        S: AsyncRead + Unpin,
    {
        let mut lines = Framed::from_parts(FramedParts::new::<()>(read, codec::JsonLinesCodec::<qmp::QapiCapabilities>::new()));

        let capabilities = lines.next().await.ok_or_else(||
            io::Error::new(io::ErrorKind::UnexpectedEof, "QMP greeting expected")
        )??;

        let lines = lines.into_parts();
        let mut read = FramedParts::new::<()>(lines.io, codec::JsonLinesCodec::new());
        read.read_buf = lines.read_buf;
        let stream = Framed::from_parts(read);

        Ok(QmpStreamNegotiation {
            stream: Self {
                stream,
            },
            write: QmpStream::new(write),
            capabilities,
        })
    }
}

#[cfg(feature = "qapi-qmp")]
impl<RW: AsyncRead + AsyncWrite> QmpStream<ReadHalf<RW>> {
    pub async fn open(stream: RW) -> io::Result<QmpStreamNegotiation<Self, QmpStream<WriteHalf<RW>>>> where RW: Unpin {
        let (r, w) = split(stream);
        Self::open_split(r, w).await
    }
}

#[cfg(feature = "qapi-qmp")]
pub struct QmpStreamNegotiation<S, W> {
    pub stream: S,
    pub write: W,
    pub capabilities: qmp::QapiCapabilities,
}

#[cfg(feature = "qapi-qmp")]
impl<S, W> QmpStreamNegotiation<S, W> where
    QapiEvents<S>: Future<Output=io::Result<()>> + Unpin,
        W: Sink<Execute<qmp::qmp_capabilities, u64>, Error=io::Error> + Unpin,
{
    pub async fn negotiate_caps<C>(self, caps: C) -> io::Result<QapiStream<S, W>> where
        C: IntoIterator<Item=qmp::QMPCapability>,
    {
        let supports_oob = self.capabilities.capabilities().any(|c| c == qmp::QMPCapability::oob);
        let shared = Arc::new(QapiShared::new(supports_oob));
        let mut events = QapiEvents {
            stream: self.stream,
            shared: shared.clone(),
        };
        let service = QapiService::new(self.write, shared);
        let caps = service.execute(qmp::qmp_capabilities {
            enable: Some(caps.into_iter().collect()),
        }).and_then(|res| future::ready(res.map_err(From::from)))
        .map_err(|err|
            io::Error::new(io::ErrorKind::Other, format!("negotiation error {:?}", err))
        ).fuse();
        futures::pin_mut!(caps);

        futures::select_biased! {
            res = caps => res.map(|_| QapiStream { events, service }),
            res = (&mut events).fuse() => {
                res?;
                Err(io::Error::new(io::ErrorKind::UnexpectedEof, "unexpected EOF when negotiating QAPI capabilities"))
            },
        }
    }

    pub async fn negotiate(self) -> io::Result<QapiStream<S, W>> {
        self.negotiate_caps(std::iter::empty()).await
    }
}

type QapiCommandMap = BTreeMap<u64, oneshot::Sender<Result<Any, spec::Error>>>;

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

    fn next_oob_id(&self) -> u64 {
        self.id_counter.fetch_add(1, Ordering::Relaxed) as _
    }

    pub fn execute<C: Command>(&self, command: C) -> impl Future<Output=Result<CommandResult<C>, W::Error>> where
        W: Sink<Execute<C, u64>, Error=io::Error> + Unpin
    {
        let oob = false;
        let id = if oob || self.shared.supports_oob {
            Some(self.next_oob_id())
        } else {
            None
        };

        let sink = self.write.clone();
        let shared = self.shared.clone();
        let command = Execute::new(command, id);

        async move {
            let mut sink = sink.lock().await;
            let receiver = shared.command_insert(id.unwrap_or_default());

            sink.send(command).await?;
            if id.is_some() {
                drop(sink)
            }

            match receiver.await {
                Ok(Ok(res)) => Ok(Ok(serde::Deserialize::deserialize(&res)?)),
                Ok(Err(e)) => Ok(Err(e)),
                Err(_cancelled) => Err(io::Error::new(io::ErrorKind::UnexpectedEof, "QAPI stream disconnected")),
            }
        }
    }

    #[cfg(feature = "qapi-qga")]
    pub fn guest_sync(&self, sync_value: u32) -> impl Future<Output=io::Result<()>> where
        W: Sink<Execute<qga::guest_sync, u64>, Error=io::Error> + Unpin
    {
        let sync_value = sync_value as isize;
        self.execute(qga::guest_sync {
            id: sync_value,
        }).and_then(move |res| future::ready(res.map_err(From::from)
            .and_then(move |res| if res == sync_value {
                Ok(())
            } else {
                Err(io::Error::new(io::ErrorKind::InvalidData, "QGA sync failed"))
            })
        ))
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

#[cfg(feature = "tower-service")]
impl<W: Sink<Execute<C, u64>, Error=io::Error> + Unpin + Send + 'static, C: Command + 'static> tower_service::Service<C> for QapiService<W> {
    type Response = CommandResult<C>;
    type Error = io::Error;
    type Future = Pin<Box<dyn Future<Output=Result<Self::Response, Self::Error>> + 'static>>;

    fn poll_ready(&mut self, _: &mut Context) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: C) -> Self::Future {
        self.execute(req).boxed()
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

    fn command_remove(&self, id: u64) -> Option<oneshot::Sender<Result<Any, spec::Error>>> {
        let mut commands = self.commands.lock().unwrap();
        commands.pending.remove(&id)
    }

    fn command_insert(&self, id: u64) -> oneshot::Receiver<Result<Any, spec::Error>> {
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
    pub fn spawn(self) -> tokio::task::JoinHandle<()> where
        Self: Future<Output=io::Result<()>> + Send + 'static,
        S: 'static
    {
        {
            let commands = self.shared.commands.lock().unwrap();
            if commands.abandoned {
                log::info!("QAPI service abandoned before spawning");
                drop(commands);
                drop(self);
                return tokio::spawn(async move { }); // ugh hacky return type
            }
            self.shared.abandoned.store(true, Ordering::Relaxed);
        }
        tokio::spawn(
            self
            .map(|res| match res {
                Ok(()) => (),
                Err(e) =>
                    log::warn!("QAPI stream closed with error {:?}", e),
            })
        )
    }
}

impl<S> Drop for QapiEvents<S> {
    fn drop(&mut self) {
        let mut commands = self.shared.commands.lock().unwrap();
        commands.pending.clear();
        commands.abandoned = true;
    }
}

fn response_id<T>(res: &spec::Response<T>, supports_oob: bool) -> io::Result<u64> {
    match (res.id().and_then(|id| id.as_u64()), supports_oob) {
        (Some(id), true) =>
            Ok(id),
        (None, false) =>
            Ok(Default::default()),
        (None, true) =>
            Err(io::Error::new(io::ErrorKind::InvalidData, format!("QAPI expected response with numeric ID, got {:?}", res.id()))),
        (Some(..), false) =>
            Err(io::Error::new(io::ErrorKind::InvalidData, format!("QAPI expected response without ID, got {:?}", res.id()))),
    }
}

fn handle_response(shared: &QapiShared, res: spec::Response<Any>) -> io::Result<()> {
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
    M: TryInto<spec::Response<Any>>,
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
impl<S: Stream<Item=io::Result<qmp::QmpMessageAny>>> Stream for QapiEvents<S> {
    type Item = io::Result<qmp::Event>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = unsafe { self.get_unchecked_mut() };
        let stream = unsafe { Pin::new_unchecked(&mut this.stream) };
        let shared = &this.shared;

        shared.poll_next(cx, |cx| Poll::Ready(match futures::ready!(stream.poll_next(cx)) {
            None => None, // eof
            Some(Err(e)) => Some(Err(e)),
            Some(Ok(qmp::QmpMessage::Event(e))) => Some(Ok(e)),
            Some(Ok(qmp::QmpMessage::Response(res))) => match handle_response(shared, res) {
                Err(e) => Some(Err(e)),
                Ok(()) => {
                    cx.waker().wake_by_ref(); // TODO: I've seen this not work with tokio?
                    return Poll::Pending
                },
            },
        }))
    }
}

/*#[cfg(feature = "qapi-qmp")]
impl<S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin> QapiService<WriteHalf<S>> {
    pub async fn open_tokio(stream: S) -> io::Result<(qmp::QapiCapabilities, Self, QapiEvents<ReadHalf<S>>)> {
        Self::open(stream).await
    }
}

#[cfg(feature = "qapi-qmp")]
impl<S: AsyncRead + AsyncWrite + Unpin> QapiService<WriteHalf<S>> {
    pub async fn open(stream: S) -> io::Result<(qmp::QapiCapabilities, Self, QapiEvents<ReadHalf<S>>)> {
        let (r, w) = tokio::io::split(stream);
        QapiService::open_split(r, w).await
    }
}

#[cfg(feature = "qapi-qga")]
impl<S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin> QapiService<WriteHalf<S>> {
    pub async fn open_tokio_qga(stream: S) -> io::Result<(Self, impl Future<Output=()>)> {
        Self::open_qga(stream).await
    }
}

#[cfg(feature = "qapi-qga")]
impl<S: AsyncRead + AsyncWrite + Unpin> QapiService<WriteHalf<S>> {
    pub async fn open_qga(stream: S) -> io::Result<(Self, impl Future<Output=()>)> {
        let (r, w) = tokio::io::split(stream);
        QapiService::open_split_qga(r, w).await
    }
}*/

/*
    pub async fn execute_oob<C: Command>(&self, command: C) -> io::Result<Result<C::Ok, spec::Error>> {
        /* TODO: should we assert C::ALLOW_OOB here and/or at the type level?
         * If oob isn't supported should we fall back to serial execution or error?
         */
        self.execute_(command, true).await
    }*/
