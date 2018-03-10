extern crate serde;
extern crate serde_json;
extern crate tokio_io;
#[macro_use]
extern crate futures;
#[macro_use]
extern crate log;
extern crate bytes;
extern crate qapi;
extern crate qapi_spec as spec;

use std::marker::PhantomData;
use std::mem::replace;
use std::{io, str};
use serde::de::DeserializeOwned;
use serde::Serialize;
use tokio_io::codec::{Framed, FramedParts};
use futures::{Future, Poll, StartSend, Async, AsyncSink, Sink, Stream};
use futures::sync::BiLock;
use futures::task::{self, Task};
use bytes::BytesMut;
use bytes::buf::FromBuf;

mod codec;

pub struct QapiFuture<C, R, S> {
    state: QapiState<C, S>,
    _marker: PhantomData<fn(C) -> R>,
}

impl<C: spec::Command, S> QapiFuture<spec::CommandSerializer<C>, spec::Response<C::Ok>, S> {
    fn new(stream: S, command: C) -> Self {
        QapiFuture {
            state: QapiState::Queue {
                inner: stream,
                value: spec::CommandSerializer(command),
            },
            _marker: Default::default(),
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

impl<C, R, S, E> Future for QapiFuture<C, spec::Response<R>, S>
    where
        S: Sink<SinkItem=Box<[u8]>, SinkError=E> + Stream<Item=BytesMut, Error=E>,
        C: Serialize,
        R: DeserializeOwned,
        io::Error: From<E>,
{
    type Item = (Result<R, spec::Error>, S);
    type Error = io::Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        trace!("QapiFuture::poll()");
        match self.state.take_value() {
            Some(v) => {
                let mut encoded = serde_json::to_vec(&v)?;
                encoded.push(b'\n');
                debug!("Encoded command {}", str::from_utf8(&encoded).unwrap_or("utf8 decoding failed"));
                // TODO: queue the vec instead of the value?
                match self.state.inner_mut().unwrap().start_send(encoded.into_boxed_slice()) {
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

                trace!("QapiFuture::poll got poll result: {:?}", poll);
                match poll {
                    Some(t) => {
                        let t = serde_json::from_slice(&t)?;
                        Ok(Async::Ready((t, self.state.take_inner().unwrap())))
                    },
                    None => Err(io::Error::new(io::ErrorKind::UnexpectedEof, "expected command response, got eof")),
                }
            },
        }
    }
}

pub fn execute<
    C: qapi::Command,
    E: From<io::Error>,
    S: Sink<SinkItem=Box<[u8]>, SinkError=E> + Stream<Item=BytesMut, Error=E>,
>(c: C, s: S) -> QapiFuture<
    spec::CommandSerializer<C>,
    spec::Response<C::Ok>,
    S
> {
    QapiFuture::new(s, c)
}

struct QapiStreamInner<S> {
    stream: S,
    #[cfg(feature = "qmp")]
    events: Vec<Box<[u8]>>,
    response: Option<BytesMut>,
    #[cfg(feature = "qmp")]
    greeting: Option<Box<[u8]>>,
    fused: bool,
    #[cfg(feature = "qmp")]
    task_events: Option<Task>,
    task_response: Option<Task>,
}

impl<S> QapiStreamInner<S> {
    fn new(s: S) -> Self {
        QapiStreamInner {
            stream: s,
            #[cfg(feature = "qmp")]
            events: Default::default(),
            response: Default::default(),
            #[cfg(feature = "qmp")]
            greeting: Default::default(),
            fused: false,
            #[cfg(feature = "qmp")]
            task_events: None,
            task_response: None,
        }
    }
}

impl<R: AsRef<[u8]>, E: From<io::Error>, S: Stream<Item=R, Error=E>> QapiStreamInner<S> {
    #[cfg(feature = "qmp")]
    fn push_event(&mut self, e: &[u8]) {
        if let Some(ref task) = self.task_events {
            self.events.push(e.to_owned().into_boxed_slice());

            task.notify()
        }
    }

    #[cfg(feature = "qmp")]
    fn push_greeting(&mut self, g: &[u8]) {
        self.greeting = Some(g.to_owned().into_boxed_slice());
        if let Some(ref task) = self.task_response {
            task.notify()
        }
    }

    #[cfg(not(feature = "qmp"))]
    fn push_event(&mut self, _: &[u8]) { }

    #[cfg(not(feature = "qmp"))]
    fn push_greeting(&mut self, _: &[u8]) { }

    fn poll(&mut self) -> Poll<(), E> {
        match try_ready!(self.stream.poll()) {
            Some(v) => {
                let v = v.as_ref();
                if v.starts_with(b"{\"QMP\":") {
                    debug!("Got greeting");
                    self.push_greeting(v);
                } else if v.starts_with(b"{\"timestamp\":") || v.starts_with(b"{\"event\":") {
                    debug!("Got event");
                    self.push_event(v);
                } else {
                    debug!("Got response");
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

    #[cfg(feature = "qmp")]
    fn greeting(&mut self) -> Poll<Option<qapi::qmp::QapiCapabilities>, E> {
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

    #[cfg(feature = "qmp")]
    fn event(&mut self) -> Poll<Option<qapi::qmp::Event>, E> {
        match self.events.pop() {
            Some(v) => {
                let v = serde_json::from_slice(v.as_ref()).map_err(io::Error::from)?;
                debug!("Decoded event {:?}", v);
                Ok(Async::Ready(Some(v)))
            },
            None => if self.fused {
                Ok(Async::Ready(None))
            } else {
                Ok(Async::NotReady)
            },
        }
    }

    fn response(&mut self) -> Poll<Option<BytesMut>, E> {
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
        let inner = QapiStreamInner::new(stream);
        // TODO: why bother with a lock here, make it generic instead
        let (inner, _) = BiLock::new(inner);

        QapiStream {
            inner: inner,
        }
    }
}

#[cfg(feature = "qmp")]
pub struct QapiEventStream<S> {
    inner: BiLock<QapiStreamInner<S>>,
}

#[cfg(feature = "qmp")]
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

#[cfg(feature = "qmp")]
impl<R: AsRef<[u8]>, E: From<io::Error>, S: Stream<Item=R, Error=E>> Stream for QapiEventStream<S> {
    type Item = qapi::qmp::Event;
    type Error = E;

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

#[cfg(feature = "qmp")]
impl<E: From<io::Error>, S: Stream<Item=BytesMut, Error=E>> QapiStream<S> {
    pub fn poll_greeting(&mut self) -> Poll<Option<qapi::qmp::QapiCapabilities>, E> {
        let mut inner = try_ready!(Ok::<_, E>(self.inner.poll_lock()));
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

impl<E: From<io::Error>, S: Stream<Item=BytesMut, Error=E>> Stream for QapiStream<S> {
    type Item = BytesMut;
    type Error = E;

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

impl<E: From<io::Error>, S: Sink<SinkItem=Box<[u8]>, SinkError=E>> Sink for QapiStream<S> {
    type SinkItem = Box<[u8]>;
    type SinkError = E;

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

pub fn data_stream<S>(stream: S) -> QapiDataStream<S> {
    Framed::from_parts(
        FramedParts {
            inner: stream,
            readbuf: Default::default(),
            writebuf: Default::default(),
        },
        codec::LineCodec,
    )
}

#[cfg(feature = "qmp")]
pub fn event_stream<S>(stream: S) -> (QapiStream<QapiDataStream<S>>, QapiEventStream<QapiDataStream<S>>) {
    QapiEventStream::new(data_stream(stream))
}

pub fn stream<S>(stream: S) -> QapiStream<QapiDataStream<S>> {
    QapiStream::new(data_stream(stream))
}

#[cfg(feature = "qmp")]
pub fn qmp_handshake<S>(stream: QapiStream<S>) -> QmpHandshake<S> {
    QmpHandshake::new(stream)
}

#[cfg(feature = "qmp")]
pub struct QmpHandshake<S> {
    state: QmpHandshakeState<S>,
}

#[cfg(feature = "qmp")]
impl<S> QmpHandshake<S> {
    pub fn new(stream: QapiStream<S>) -> Self {
        QmpHandshake {
            state: QmpHandshakeState::Greeting {
                stream: stream,
            },
        }
    }
}

#[cfg(feature = "qmp")]
enum QmpHandshakeState<S> {
    None,
    Greeting {
        stream: QapiStream<S>,
    },
    Future {
        greeting: Option<qapi::qmp::QMP>,
        future: QapiFuture<
            spec::CommandSerializer<qapi::qmp::qmp_capabilities>,
            spec::Response<spec::Empty>,
            QapiStream<S>,
        >,
    },
}

#[cfg(feature = "qmp")]
impl<E: From<io::Error>, S> Future for QmpHandshake<S> where
    S : Stream<Item=BytesMut, Error=E> + Sink<SinkItem=Box<[u8]>, SinkError=E>,
    io::Error: From<E>,
{
    type Item = (qapi::qmp::QMP, QapiStream<S>);
    type Error = E;

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
            future: QapiFuture::new(stream, qapi::qmp::qmp_capabilities { }),
        };

        self.poll()
    }
}

#[cfg(feature = "qga")]
pub fn qga_handshake<S>(stream: QapiStream<S>) -> QgaHandshake<S> {
    let sync = &stream as *const _ as usize as _;
    QgaHandshake::new(stream, sync)
}

#[cfg(feature = "qga")]
pub struct QgaHandshake<S> {
    expected: isize,
    future: QapiFuture<
        spec::CommandSerializer<qapi::qga::guest_sync>,
        spec::Response<isize>,
        QapiStream<S>,
    >,
}

#[cfg(feature = "qga")]
impl<S> QgaHandshake<S> {
    pub fn new(stream: QapiStream<S>, sync_value: isize) -> Self {
        QgaHandshake {
            expected: sync_value,
            future: QapiFuture::new(stream, qapi::qga::guest_sync { id: sync_value }),
        }
    }
}

#[cfg(feature = "qga")]
impl<E: From<io::Error>, S> Future for QgaHandshake<S> where
    S : Stream<Item=BytesMut, Error=E> + Sink<SinkItem=Box<[u8]>, SinkError=E>,
    io::Error: From<E>,
{
    type Item = QapiStream<S>;
    type Error = E;

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
