#![doc(html_root_url = "http://docs.rs/tokio-qapi/0.4.0")]
#![feature(futures_api, async_await, await_macro)]

use qapi_spec as spec;

#[cfg(feature = "qapi-qmp")]
pub use qapi_qmp as qmp;

#[cfg(feature = "qapi-qga")]
pub use qapi_qga as qga;

pub use qapi_spec::{Any, Dictionary, Empty, Command, Event, Error, ErrorClass, Timestamp};

use std::mem::replace;
use std::{io, str, usize};
use tokio_codec::{Framed, LinesCodec, Encoder, Decoder};
use tokio_io::{AsyncRead, AsyncWrite};
use futures::{Future, Poll, Sink, Stream, StreamExt, try_ready};
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
        } {
            Ok(None)
        }
    }

    fn decode_eof(&mut self, buf: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        self.lines.decode_eof(buf)
    }
}

// dunno what I'm doing but this shit should typecheck:
// yeah um just wait for tokio to support futures-preview, the following should work eventually
pub struct QapiFrames<S> {
    inner: Framed<S, LinesCodec>,
}
impl<S: AsyncRead + AsyncWrite> QapiFrames<S> {
    pub fn new(stream: S) -> Self {
        QapiFrames {
            inner: Framed::new(stream, LinesCodec::new_with_max_length(usize::MAX)),
        }
    }
}
fn assert_stream<F: Stream>(f: F) { unimplemented!() }
fn testing_shit<S: AsyncRead + AsyncWrite, U: Decoder + Encoder>(s: S, u: U) {
    let f = Framed::new(s, u);
    assert_stream(f);
}
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
