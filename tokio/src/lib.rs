#![doc(html_root_url = "http://docs.rs/tokio-qapi/0.5.0")]
#![feature(futures_api, async_await, await_macro, arbitrary_self_types)]

use qapi_spec as spec;

#[cfg(feature = "qapi-qmp")]
pub use qapi_qmp as qmp;

#[cfg(feature = "qapi-qga")]
pub use qapi_qga as qga;

pub use qapi_spec::{Any, Dictionary, Empty, Command, Event, Error, ErrorClass, Timestamp};

use std::collections::BTreeMap;
use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};
use std::mem::replace;
use std::{io, str, usize};
use result::OptionResultExt;
use futures::compat::{Stream01CompatExt, Future01CompatExt, Compat01As03, Compat};
use futures::channel::oneshot;
use futures::io::{AsyncRead, AsyncWrite, AsyncReadExt, AsyncWriteExt, ReadHalf, WriteHalf};
use tokio_codec::{Framed, FramedRead, LinesCodec, Encoder, Decoder};
use futures::{Future, TryFutureExt, Poll, Sink, Stream, StreamExt, future, try_ready, try_join};
use futures::stream::{FusedStream, unfold};
use std::pin::Pin;
use std::marker::Unpin;
//use futures::io::{AsyncRead as _AsyncRead, AsyncWrite as _AsyncWrite, AsyncReadExt, ReadHalf, WriteHalf};
use futures::lock::Mutex;
//use futures::sync::BiLock;
//use futures::task::{self, Task};
use bytes::BytesMut;
use bytes::buf::FromBuf;
use log::{trace, debug};


pub struct QapiCodec {
    lines: LinesCodec, // XXX: Item=String, maybe use a Vec<u8> after all
}

impl Encoder for QapiCodec {
    type Item = ();
    type Error = io::Error;

    fn encode(&mut self, item: Self::Item, dst: &mut BytesMut) -> Result<(), Self::Error> {
        unimplemented!()
    }
}

#[cfg(feature = "qapi-qmp")]
impl Decoder for QapiCodec {
    type Item = qapi_qmp::QmpMessage<Any>;
    type Error = io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        if let Some(line) = self.lines.decode(src)? {
            let line = serde_json::from_str::<qapi_qmp::QmpMessage<Any>>(&line)?;
            Ok(Some(line))
        } else {
            Ok(None)
        }
    }

    /*fn decode_eof(&mut self, buf: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        self.lines.decode_eof(buf)
    }*/
}

pub struct QapiFrames<S> {
    inner: Framed<S, LinesCodec>,
}

/*
impl<S: AsyncRead + AsyncWrite> QapiFrames<S> {
    pub fn new(stream: S) -> Self {
        QapiFrames {
            inner: Framed::new(stream, LinesCodec::new_with_max_length(usize::MAX)),
        }
    }
}

pub fn assert_stream<F: Stream>(f: F) { unimplemented!() }
pub fn testing_shit<S: AsyncRead + AsyncWrite, U: Decoder + Encoder>(s: S, u: U) {
    let f = Framed::new(s, u);
    assert_stream(f.compat());
}*/

/*
#[cfg(feature = "qapi-qmp")]
pub async fn greeting<S: AsyncRead>(frames: QapiFrames<S>) -> Result<(qmp::QapiCapabilities, QapiFrames<S>), io::Error> {
    match await!(frames.inner.next()) {
        Some(qmp::QmpMessage::Greeting(greeting)) => Ok((greeting, frames)),
        Some(e) => Err(io::Error::new(io::ErrorKind::InvalidData, format!("expected QMP greeting, got {:?}", e))),
        None => Err(io::Error::new(io::ErrorKind::UnexpectedEof, "expected QMP greeting, got EOF")),
    }
}*/

/*
pub struct QapiFuture<C, S> {
    state: QapiState<C, S>,
}

impl<C: Command, S> QapiFuture<C, S> {
    pub fn new(stream: S, command: C) -> Self {
        QapiFuture {
            state: QapiState::Queue {
                inner: stream,
                value: command,
            },
        }
    }
}

enum QapiState<C, S> {
    Queue {
        inner: S,
        value: C,
    },
    Waiting {
        inner: S,
    },
    None,
}

impl<C, S> QapiState<C, S> {
    fn inner_mut(&mut self) -> Option<&mut S> {
        match *self {
            QapiState::Queue { ref mut inner, .. } => Some(inner),
            QapiState::Waiting { ref mut inner } => Some(inner),
            QapiState::None => None,
        }
    }

    fn take_value(&mut self) -> Option<C> {
        match replace(self, QapiState::None) {
            QapiState::Queue { inner, value } => {
                *self = QapiState::Waiting { inner: inner };
                Some(value)
            },
            v @ QapiState::Waiting { .. } => {
                *self = v;
                None
            },
            QapiState::None => None,
        }
    }

    fn take_inner(&mut self) -> Option<S> {
        match replace(self, QapiState::None) {
            QapiState::Queue { inner, .. } => {
                Some(inner)
            },
            QapiState::Waiting { inner, .. } => {
                Some(inner)
            },
            QapiState::None => None,
        }
    }

    fn set_value(&mut self, v: C) {
        match replace(self, QapiState::None) {
            QapiState::Queue { inner, .. } => {
                *self = QapiState::Queue { inner: inner, value: v };
            },
            QapiState::Waiting { inner } => {
                *self = QapiState::Queue { inner: inner, value: v };
            },
            QapiState::None => unreachable!(),
        }
    }
}

impl<C, S, E> Future for QapiFuture<C, S>
    where
        S: Sink<SinkItem=Box<[u8]>, SinkError=E> + Stream<Error=E>,
        S::Item: AsRef<[u8]>,
        C: Command,
        io::Error: From<E>,
{
    type Item = (Result<C::Ok, Error>, S);
    type Error = io::Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        trace!("QapiFuture::poll()");
        match self.state.take_value() {
            Some(v) => {
                let encoded = encode_command(&v)?;
                debug!("-> {}", str::from_utf8(&encoded).unwrap_or("utf8 decoding failed"));
                // TODO: queue the vec instead of the value?
                match self.state.inner_mut().unwrap().start_send(encoded) {
                    Ok(AsyncSink::Ready) => self.poll(),
                    Ok(AsyncSink::NotReady(..)) => {
                        trace!("Failed to start_send, try later");
                        self.state.set_value(v);
                        Ok(Async::NotReady)
                    },
                    Err(e) => Err(e.into()),
                }
            },
            None => {
                let poll = if let Some(inner) = self.state.inner_mut() {
                    trace!("QapiFuture::poll_complete");
                    try_ready!(inner.poll_complete());

                    trace!("QapiFuture::poll for data");
                    try_ready!(inner.poll())
                } else {
                    panic!("future polled after returning value")
                };

                match poll {
                    Some(t) => {
                        let t = t.as_ref();
                        let t: qapi_spec::Response<C::Ok> = serde_json::from_slice(&t)?;
                        Ok(Async::Ready((t.result(), self.state.take_inner().unwrap())))
                    },
                    None => Err(io::Error::new(io::ErrorKind::UnexpectedEof, "expected command response, got eof")),
                }
            },
        }
    }
}

struct QapiStreamInner<S> {
    stream: S,
    #[cfg(feature = "qapi-qmp")]
    events: Vec<Box<[u8]>>,
    response: Option<BytesMut>,
    #[cfg(feature = "qapi-qmp")]
    greeting: Option<Box<[u8]>>,
    fused: bool,
    fused_events: bool,
    #[cfg(feature = "qapi-qmp")]
    task_events: Option<Task>,
    task_response: Option<Task>,
}

impl<S> QapiStreamInner<S> {
    fn new(s: S) -> Self {
        QapiStreamInner {
            stream: s,
            #[cfg(feature = "qapi-qmp")]
            events: Default::default(),
            response: Default::default(),
            #[cfg(feature = "qapi-qmp")]
            greeting: Default::default(),
            fused: false,
            fused_events: false,
            #[cfg(feature = "qapi-qmp")]
            task_events: None,
            task_response: None,
        }
    }
}

impl<S> QapiStreamInner<S> where
    S: Stream,
    S::Item: AsRef<[u8]>,
    S::Error: From<io::Error>,
{
    #[cfg(feature = "qapi-qmp")]
    fn push_event(&mut self, e: &[u8]) {
        if self.fused_events {
            return
        }

        if let Some(ref task) = self.task_events {
            self.events.push(e.to_owned().into_boxed_slice());

            task.notify()
        }
    }

    #[cfg(feature = "qapi-qmp")]
    fn push_greeting(&mut self, g: &[u8]) {
        self.greeting = Some(g.to_owned().into_boxed_slice());
        if let Some(ref task) = self.task_response {
            task.notify()
        }
    }

    #[cfg(not(feature = "qapi-qmp"))]
    fn push_event(&mut self, _: &[u8]) { }

    #[cfg(not(feature = "qapi-qmp"))]
    fn push_greeting(&mut self, _: &[u8]) { }

    fn poll(&mut self) -> Poll<(), S::Error> {
        match try_ready!(self.stream.poll()) {
            Some(v) => {
                let v = v.as_ref();
                debug!("<- {}", str::from_utf8(v).unwrap_or("utf8 decoding failed"));
                if v.starts_with(b"{\"QMP\":") {
                    self.push_greeting(v);
                } else if v.starts_with(b"{\"timestamp\":") || v.starts_with(b"{\"event\":") {
                    self.push_event(v);
                } else {
                    self.response = Some(BytesMut::from_buf(v));
                    if let Some(ref task) = self.task_response {
                        task.notify()
                    }
                }

                Ok(Async::Ready(()))
            },
            None => {
                self.fused = true;
                Ok(Async::Ready(()))
            },
        }
    }

    #[cfg(feature = "qapi-qmp")]
    fn greeting(&mut self) -> Poll<Option<qmp::QapiCapabilities>, S::Error> {
        match self.greeting.take() {
            Some(g) => {
                serde_json::from_slice(&g)
                    .map_err(io::Error::from).map_err(From::from)
                    .map(Async::Ready)
            },
            None => if self.fused {
                Ok(Async::Ready(None))
            } else {
                Ok(Async::NotReady)
            },
        }
    }

    #[cfg(feature = "qapi-qmp")]
    fn event(&mut self) -> Poll<Option<qmp::Event>, S::Error> {
        match self.events.pop() {
            Some(v) => {
                let v = serde_json::from_slice(v.as_ref()).map_err(io::Error::from)?;
                Ok(Async::Ready(Some(v)))
            },
            None => if self.fused || self.fused_events {
                Ok(Async::Ready(None))
            } else {
                Ok(Async::NotReady)
            },
        }
    }

    fn response(&mut self) -> Poll<Option<BytesMut>, S::Error> {
        match self.response.take() {
            Some(v) => {
                Ok(Async::Ready(Some(v)))
            },
            None => if self.fused {
                Ok(Async::Ready(None))
            } else {
                Ok(Async::NotReady)
            },
        }
    }
}

pub struct QapiStream<S> {
    inner: BiLock<QapiStreamInner<S>>,
}

impl<S> QapiStream<S> {
    pub fn new(stream: S) -> Self {
        let mut inner = QapiStreamInner::new(stream);
        inner.fused_events = true;
        // TODO: why bother with a lock here, make it generic instead
        let (inner, _) = BiLock::new(inner);

        QapiStream {
            inner: inner,
        }
    }

    pub fn execute<C: Command>(self, command: C) -> QapiFuture<C, Self> {
        QapiFuture::new(self, command)
    }
}

#[cfg(feature = "qapi-qmp")]
pub struct QapiEventStream<S> {
    inner: BiLock<QapiStreamInner<S>>,
}

#[cfg(feature = "qapi-qmp")]
impl<S> QapiEventStream<S> {
    pub fn new(stream: S) -> (QapiStream<S>, QapiEventStream<S>) {
        let inner = QapiStreamInner::new(stream);
        let (inner0, inner1) = BiLock::new(inner);
        (
            QapiStream {
                inner: inner0,
            },
            QapiEventStream {
                inner: inner1,
            }
        )
    }
}

#[cfg(feature = "qapi-qmp")]
impl<S> Stream for QapiEventStream<S> where
    S: Stream,
    S::Item: AsRef<[u8]>,
    S::Error: From<io::Error>,
{
    type Item = qmp::Event;
    type Error = S::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        let mut inner = try_ready!(Ok::<_, Self::Error>(self.inner.poll_lock()));
        inner.task_events = Some(task::current());
        let _ = inner.poll()?;
        match inner.event() {
            Ok(Async::NotReady) => {
                try_ready!(inner.poll());
                inner.event()
            },
            v => v,
        }
    }
}

#[cfg(feature = "qapi-qmp")]
impl<S> QapiStream<S> where
    S: Stream,
    S::Item: AsRef<[u8]>,
    S::Error: From<io::Error>,
{
    pub fn poll_greeting(&mut self) -> Poll<Option<qmp::QapiCapabilities>, S::Error> {
        let mut inner = try_ready!(Ok::<_, S::Error>(self.inner.poll_lock()));
        inner.task_response = Some(task::current());
        let _ = inner.poll()?;
        match inner.greeting() {
            Ok(Async::NotReady) => {
                try_ready!(inner.poll());
                inner.greeting()
            },
            v => v,
        }
    }
}

impl<S> Stream for QapiStream<S> where
    S: Stream,
    S::Item: AsRef<[u8]>,
    S::Error: From<io::Error>,
{
    type Item = BytesMut;
    type Error = S::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        trace!("QapiStream::poll()");
        let mut inner = try_ready!(Ok::<_, Self::Error>(self.inner.poll_lock()));
        inner.task_response = Some(task::current());
        let _ = inner.poll()?;
        match inner.response() {
            Ok(Async::NotReady) => {
                try_ready!(inner.poll());
                inner.response()
            },
            v => v,
        }
    }
}

impl<S> Sink for QapiStream<S> where
    S: Sink<SinkItem = Box<[u8]>>,
    S::SinkError: From<io::Error>,
{
    type SinkItem = Box<[u8]>;
    type SinkError = S::SinkError;

    fn start_send(&mut self, item: Self::SinkItem) -> StartSend<Self::SinkItem, Self::SinkError> {
        trace!("QapiStream::start_send()");
        let mut inner = match self.inner.poll_lock() {
            Async::Ready(inner) => inner,
            Async::NotReady => return Ok(AsyncSink::NotReady(item)),
        };

        inner.stream.start_send(item)
    }

    fn poll_complete(&mut self) -> Poll<(), Self::SinkError> {
        trace!("QapiStream::poll_complete()");
        let mut inner = try_ready!(Ok::<_, Self::SinkError>(self.inner.poll_lock()));
        inner.stream.poll_complete()
    }
}

pub type QapiDataStream<S> = Framed<S, codec::LineCodec>;

pub fn data_stream<S: AsyncRead + AsyncWrite>(stream: S) -> QapiDataStream<S> {
    Framed::new(stream, codec::LineCodec)
}

#[cfg(feature = "qapi-qmp")]
pub fn event_stream<S: AsyncRead + AsyncWrite>(stream: S) -> (QapiStream<QapiDataStream<S>>, QapiEventStream<QapiDataStream<S>>) {
    QapiEventStream::new(data_stream(stream))
}

pub fn stream<S: AsyncRead + AsyncWrite>(stream: S) -> QapiStream<QapiDataStream<S>> {
    QapiStream::new(data_stream(stream))
}

#[cfg(feature = "qapi-qmp")]
pub fn qmp_handshake<S>(stream: QapiStream<S>) -> QmpHandshake<S> {
    QmpHandshake::new(stream)
}

#[cfg(feature = "qapi-qmp")]
pub struct QmpHandshake<S> {
    state: QmpHandshakeState<S>,
}

#[cfg(feature = "qapi-qmp")]
impl<S> QmpHandshake<S> {
    pub fn new(stream: QapiStream<S>) -> Self {
        QmpHandshake {
            state: QmpHandshakeState::Greeting {
                stream: stream,
            },
        }
    }
}

#[cfg(feature = "qapi-qmp")]
enum QmpHandshakeState<S> {
    None,
    Greeting {
        stream: QapiStream<S>,
    },
    Future {
        greeting: Option<qmp::QMP>,
        future: QapiFuture<qmp::qmp_capabilities, QapiStream<S>>,
    },
}

#[cfg(feature = "qapi-qmp")]
impl<S, E> Future for QmpHandshake<S> where
    S: Stream<Error=E> + Sink<SinkItem=Box<[u8]>, SinkError=E>,
    S::Item: AsRef<[u8]>,
    S::Error: From<io::Error>,
    io::Error: From<S::Error>,
{
    type Item = (qmp::QMP, QapiStream<S>);
    type Error = S::Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let g = match self.state {
            QmpHandshakeState::Greeting { ref mut stream } => {
                try_ready!(stream.poll_greeting())
            },
            QmpHandshakeState::Future { ref mut future, ref mut greeting } => {
                let (res, stream) = try_ready!(future.poll());
                if let Err(e) = res { // weird type gymnastics here ._.
                    let err: io::Error = From::from(e);
                    return Err(err.into())
                }
                let greeting = greeting.take().unwrap();
                return Ok(Async::Ready((greeting, stream)))
            },
            QmpHandshakeState::None => unreachable!(),
        };

        let g = match g {
            Some(g) => g,
            None => return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "expected handshake greeting").into()),
        };

        let stream = match replace(&mut self.state, QmpHandshakeState::None) {
            QmpHandshakeState::Greeting { stream } => stream,
            _ => unreachable!(),
        };

        self.state = QmpHandshakeState::Future {
            greeting: Some(g.QMP),
            future: QapiFuture::new(stream, qmp::qmp_capabilities { enable: None }),
        };

        self.poll()
    }
}

#[cfg(feature = "qapi-qga")]
pub fn qga_handshake<S>(stream: QapiStream<S>) -> QgaHandshake<S> {
    let sync = &stream as *const _ as usize as _;
    QgaHandshake::new(stream, sync)
}

#[cfg(feature = "qapi-qga")]
pub struct QgaHandshake<S> {
    expected: isize,
    future: QapiFuture<qga::guest_sync, QapiStream<S>>,
}

#[cfg(feature = "qapi-qga")]
impl<S> QgaHandshake<S> {
    pub fn new(stream: QapiStream<S>, sync_value: isize) -> Self {
        QgaHandshake {
            expected: sync_value,
            future: QapiFuture::new(stream, qga::guest_sync { id: sync_value }),
        }
    }
}

#[cfg(feature = "qapi-qga")]
impl<S, E> Future for QgaHandshake<S> where
    S: Stream<Error=E> + Sink<SinkItem=Box<[u8]>, SinkError=E>,
    S::Item: AsRef<[u8]>,
    S::Error: From<io::Error>,
    io::Error: From<S::Error>,
{
    type Item = QapiStream<S>;
    type Error = S::Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let (r, stream) = try_ready!(self.future.poll());
        match r {
            Ok(r) if r == self.expected => Ok(Async::Ready(stream)),
            Ok(..) => Err(io::Error::new(io::ErrorKind::InvalidData, "guest-sync handshake failed").into()),
            Err(e) => { // weird type gymnastics here ._.
                let err: io::Error = From::from(e);
                Err(err.into())
            },
        }
    }
}

pub fn encode_command<C: Command>(c: &C) -> io::Result<Box<[u8]>> {
    let mut encoded = serde_json::to_vec(&qapi_spec::CommandSerializerRef(c))?;
    encoded.push(b'\n');
    Ok(encoded.into_boxed_slice())
}*/

// NEW SKETCH

type QapiStreamLines<S> = Compat01As03<FramedRead<Compat<S>, LinesCodec>>;

type QapiCommandMap = BTreeMap<u64, oneshot::Sender<Result<Any, qapi_spec::Error>>>;

pub struct QapiStream<W> {
    pending: QapiShared,
    write_lock: Mutex<W>,
    supports_oob: bool,
    id_counter: AtomicUsize,
}

impl<W> QapiStream<W> {
    fn new(write: W, pending: QapiShared, supports_oob: bool) -> Self {
        QapiStream {
            pending,
            write_lock: Mutex::new(write),
            supports_oob,
            id_counter: AtomicUsize::new(0),
        }
    }

    fn next_oob_id(&self) -> u64 {
        self.id_counter.fetch_add(1, Ordering::Relaxed) as _
    }
}

type QapiShared = Arc<Mutex<QapiCommandMap>>;

#[cfg(feature = "qapi-qmp")]
pub struct QapiEvents<R> {
    lines: QapiStreamLines<R>,
    pending: QapiShared,
    supports_oob: bool,
}

#[cfg(feature = "qapi-qmp")]
impl<R> QapiEvents<R> {
    fn new(lines: QapiStreamLines<R>, supports_oob: bool) -> (Self, QapiShared) {
        let pending: QapiShared = Arc::new(Mutex::new(Default::default()));

        (QapiEvents {
            lines,
            pending: pending.clone(),
            supports_oob,
        }, pending)
    }
}

#[cfg(feature = "qapi-qmp")]
enum QapiEventsMessage {
    Response {
        id: u64,
    },
    Event(qapi_qmp::Event),
    Eof,
}

#[cfg(feature = "qapi-qmp")]
impl<R: AsyncRead> QapiEvents<R> {
    async fn process_response(self_supports_oob: bool, self_pending: &QapiShared, res: qapi_spec::Response<Any>) -> io::Result<u64> {
        let id = match (res.id().and_then(|id| id.as_u64()), self_supports_oob) {
            (Some(id), true) => id,
            (None, false) => Default::default(),
            (None, true) => return Err(io::Error::new(io::ErrorKind::InvalidData, format!("QAPI expected response with numeric ID, got {:?}", res.id()))),
            (Some(..), false) => return Err(io::Error::new(io::ErrorKind::InvalidData, format!("QAPI expected response without ID, got {:?}", res.id()))),
        };
        let mut pending = await!(self_pending.lock());
        if let Some(sender) = pending.remove(&id) {
            drop(pending);
            sender.send(res.result())
                .map_err(|e| unimplemented!())
                .map(|()| id)
        } else {
            Err(io::Error::new(io::ErrorKind::InvalidData, format!("unknown QAPI response with ID {:?}", res.id())))
        }
    }

    async fn process_message(&mut self) -> io::Result<QapiEventsMessage> {
        let msg = match await!(self.lines.next()).invert()? {
            Some(line) => serde_json::from_str::<qapi_qmp::QmpMessage<Any>>(&line)?,
            None => return Ok(QapiEventsMessage::Eof),
        };
        match msg {
            qapi_qmp::QmpMessage::Event(event) => Ok(QapiEventsMessage::Event(event)),
            //calling self here makes this async fn !Send because Compat is !Sync and it will capture &Self
            qapi_qmp::QmpMessage::Response(res) => {
                let id = await!(Self::process_response(self.supports_oob, &self.pending, res))?;
                Ok(QapiEventsMessage::Response { id })
            },
        }
    }

    pub async fn next_event(&mut self) -> io::Result<Option<qapi_qmp::Event>> {
        loop {
            match await!(self.process_message())? {
                QapiEventsMessage::Response { .. } => (),
                QapiEventsMessage::Event(event) => break Ok(Some(event)),
                QapiEventsMessage::Eof => break Ok(None),
            }
        }
    }

    pub fn into_stream(self) -> impl Stream<Item=io::Result<qapi_qmp::Event>> + FusedStream {
        unfold(self, async move |mut s| {
            await!(s.next_event()).invert().map(|r| (r, s))
        })
    }

    pub async fn spin(mut self) {
        while let Some(res) = await!(self.next_event()).invert() {
            match res {
                Ok(event) => trace!("QapiEvents::spin ignoring event: {:#?}", event),
                Err(err) => trace!("QapiEvents::spin ignoring error: {:#?}", err),
            }
        }
    }
}

#[cfg(feature = "qapi-qmp")]
impl<S: tokio_io::AsyncRead + tokio_io::AsyncWrite> QapiStream<WriteHalf<Compat01As03<S>>> {
    pub async fn open_tokio(stream: S) -> io::Result<(qmp::QapiCapabilities, Self, QapiEvents<ReadHalf<Compat01As03<S>>>)> {
        await!(Self::open(Compat01As03::new(stream)))
    }
}

#[cfg(feature = "qapi-qmp")]
impl<S: AsyncRead + AsyncWrite> QapiStream<WriteHalf<S>> {
    pub async fn open(stream: S) -> io::Result<(qmp::QapiCapabilities, Self, QapiEvents<ReadHalf<S>>)> {
        let (r, w) = stream.split();
        await!(QapiStream::open_split(r, w))
    }
}

#[cfg(feature = "qapi-qga")]
impl<S: tokio_io::AsyncRead + tokio_io::AsyncWrite> QapiStream<WriteHalf<Compat01As03<S>>> {
    pub async fn open_tokio_qga(stream: S) -> io::Result<(Self, impl Future<Output=()>)> {
        await!(Self::open_qga(Compat01As03::new(stream)))
    }
}

#[cfg(feature = "qapi-qga")]
impl<S: AsyncRead + AsyncWrite> QapiStream<WriteHalf<S>> {
    pub async fn open_qga(stream: S) -> io::Result<(Self, impl Future<Output=()>)> {
        let (r, w) = stream.split();
        await!(QapiStream::open_split_qga(r, w))
    }
}

#[cfg(feature = "qapi-qmp")]
impl<W: AsyncWrite + Unpin> QapiStream<W> {
    pub async fn open_split<R: AsyncRead>(read: R, write: W) -> io::Result<(qmp::QapiCapabilities, Self, QapiEvents<R>)> {
        let mut lines = FramedRead::new(Compat::new(read), LinesCodec::new()).compat();

        let greeting = await!(lines.next()).ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "blah"))??;
        let greeting = serde_json::from_str::<qmp::QapiCapabilities>(&greeting)?;
        let caps = greeting.capabilities();

        let supports_oob = caps.iter().any(|&c| c == qmp::QMPCapability::oob);
        let (mut events, pending) = QapiEvents::new(lines, supports_oob);
        let stream = QapiStream::new(write, pending, supports_oob);

        let mut caps = Vec::new();
        if supports_oob {
            caps.push(qmp::QMPCapability::oob);
        }

        await!(stream.negotiate_caps(&mut events, caps))?;

        Ok((greeting, stream, events))
    }

    async fn negotiate_caps<'a, R: AsyncRead>(&'a self, events: &'a mut QapiEvents<R>, caps: Vec<qmp::QMPCapability>) -> io::Result<()> {
        let caps = self.execute(qmp::qmp_capabilities {
            enable: Some(caps),
        }).and_then(|res| future::ready(res.map_err(From::from))).map_err(|err| unimplemented!("negotiation error {:?}", err));
        let events = events.process_message().and_then(|msg| future::ready(match msg {
            QapiEventsMessage::Response { id } => Ok(()),
            QapiEventsMessage::Eof =>
                Err(io::Error::new(io::ErrorKind::UnexpectedEof, "unexpected EOF when negotiating QAPI capabilities")),
            QapiEventsMessage::Event(event) =>
                Err(unimplemented!("unexpected event {:?}", event)),
        }));
        try_join!(caps, events).map(|(spec::Empty { }, ())| ())
    }
}

#[cfg(feature = "qapi-qga")]
impl<W: AsyncWrite + Unpin> QapiStream<W> {
    pub async fn open_split_qga<R: AsyncRead>(read: R, write: W) -> io::Result<(Self, impl Future<Output=()>)> {
        let mut lines = FramedRead::new(Compat::new(read), LinesCodec::new()).compat();

        let supports_oob = false;
        let (mut events, pending) = QapiEvents::new(lines, supports_oob);
        let stream = QapiStream::new(write, pending, supports_oob);

        let sync_value = &stream as *const _ as usize as _; // great randomness here um
        await!(stream.guest_sync(&mut events, sync_value))?;

        // TODO: spin will hold on to the shared reference forever ._.
        Ok((stream, events.spin()))
    }

    async fn guest_sync<'a, R: AsyncRead>(&'a self, events: &'a mut QapiEvents<R>, sync_value: u32) -> io::Result<()> {
        let sync_value = sync_value as isize;
        let sync = self.execute(qga::guest_sync {
            id: sync_value,
        }).map_err(|err| unimplemented!("negotiation error {:?}", err))
        .and_then(|res| future::ready(res.map_err(From::from)))
        .and_then(|res| future::ready(if res == sync_value {
            Ok(())
        } else {
            Err(io::Error::new(io::ErrorKind::InvalidData, "QMP sync failed"))
        }));

        let events = events.process_message().and_then(|msg| future::ready(match msg {
            QapiEventsMessage::Response { id } => Ok(()),
            QapiEventsMessage::Eof =>
                Err(io::Error::new(io::ErrorKind::UnexpectedEof, "unexpected EOF when syncing QMP connection")),
            QapiEventsMessage::Event(event) =>
                Err(io::Error::new(io::ErrorKind::InvalidData, format!("unexpected QMP event: {:?}", event))),
        }));

        try_join!(sync, events).map(|((), ())| ())
    }
}


#[cfg(feature = "qapi-qmp")]
impl<W: AsyncWrite> QapiStream<W> {
    pub async fn execute<'a, C: Command + 'a>(self: &'a Self, command: C) -> io::Result<Result<C::Ok, qapi_spec::Error>> {
        await!(self.execute_(command, false))
    }

    pub async fn execute_oob<'a, C: Command + 'a>(self: &'a Self, command: C) -> io::Result<Result<C::Ok, qapi_spec::Error>> {
        /* TODO: should we assert C::ALLOW_OOB here and/or at the type level?
         * If oob isn't supported should we fall back to serial execution or error?
         */
        await!(self.execute_(command, true))
    }

    async fn execute_<'a, C: Command + 'a>(self: &'a Self, command: C, oob: bool) -> io::Result<Result<C::Ok, qapi_spec::Error>> {
        let (id, mut write, mut encoded) = if self.supports_oob {
            let id = self.next_oob_id();
            (
                Some(id),
                await!(self.write_lock.lock()),
                serde_json::to_vec(&qapi_spec::CommandSerializerRef::with_id(&command, id, oob))?,
            )
        } else {
            (
                None,
                await!(self.write_lock.lock()),
                serde_json::to_vec(&qapi_spec::CommandSerializerRef::new(&command, false))?,
            )
        };

        encoded.push(b'\n');
        await!(write.write_all(&encoded))?;

        if id.is_some() {
            // command mutex is unnecessary when protocol supports oob ids
            drop(write)
        }

        let (sender, receiver) = oneshot::channel();
        {
            let mut pending = await!(self.pending.lock());
            if let Some(prev) = pending.insert(id.unwrap_or(Default::default()), sender) {
                panic!("QAPI duplicate command id {:?}, this should not happen", prev);
            }
        }

        match await!(receiver) {
            Ok(Ok(res)) => Ok(Ok(serde::Deserialize::deserialize(&res)?)),
            Ok(Err(e)) => Ok(Err(e)),
            Err(_cancelled) => Err(io::Error::new(io::ErrorKind::UnexpectedEof, "QAPI stream disconnected")),
        }
    }

    pub async fn close(self) -> io::Result<()> {
        // forcefully stop event streams and spin() so the socket can be closed
        unimplemented!();
    }
}
