use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::sync::Arc;
use futures::{Sink, Stream};
use tokio::io::{AsyncRead, AsyncWrite, ReadHalf, WriteHalf, split};
use tokio_util::codec::{Framed, FramedParts};
use qapi_spec::{Execute, Response, Any};
#[cfg(feature = "qapi-qmp")]
use qapi_qmp::{QmpMessageAny, QmpCommand, QapiCapabilities, QMPCapability};
#[cfg(feature = "qapi-qmp")]
use super::QmpStreamNegotiation;
use super::{codec::JsonLinesCodec, QapiEvents, QapiService, QapiStream, QapiShared};

pub struct QgaStreamTokio<S> {
    stream: Framed<S, JsonLinesCodec<Response<Any>>>
}

impl<S> QgaStreamTokio<S> {
    fn new(stream: S) -> Self {
        Self {
            stream: Framed::from_parts(FramedParts::new::<()>(stream, JsonLinesCodec::new())),
        }
    }

    fn pair<W>(self, write: W) -> QapiStream<Self, W> {
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

    pub fn open_split<W>(read: S, write: W) -> QapiStream<Self, QgaStreamTokio<W>> {
        let r = Self::new(read);
        let w = QgaStreamTokio::new(write);

        r.pair(w)
    }
}

impl<R> QgaStreamTokio<ReadHalf<R>> {
    pub fn open(stream: R) -> QapiStream<Self, QgaStreamTokio<WriteHalf<R>>> where
        R: AsyncRead + AsyncWrite,
    {
        let (r, w) = split(stream);
        let r = Self::new(r);
        let w = QgaStreamTokio::new(w);

        r.pair(w)
    }
}

#[cfg(feature = "async-tokio-uds")]
impl QgaStreamTokio<ReadHalf<tokio::net::UnixStream>> {
    pub async fn open_uds<P: AsRef<std::path::Path>>(socket_addr: P) -> io::Result<QapiStream<Self, QgaStreamTokio<WriteHalf<tokio::net::UnixStream>>>> {
        let socket = tokio::net::UnixStream::connect(socket_addr).await?;
        let (r, w) = split(socket);
        Ok(Self::open_split(r, w))
    }
}

impl<S> QgaStreamTokio<S> {
    fn stream(self: Pin<&mut Self>) -> Pin<&mut Framed<S, JsonLinesCodec<Response<Any>>>> {
        unsafe {
            self.map_unchecked_mut(|this| &mut this.stream)
        }
    }
}

impl<S: AsyncRead> Stream for QgaStreamTokio<S> {
    type Item = io::Result<Response<Any>>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        self.stream().poll_next(cx)
    }
}

#[cfg(feature = "qapi-qga")]
impl<S: AsyncWrite, C: qapi_qga::QgaCommand, I: serde::Serialize> Sink<Execute<C, I>> for QgaStreamTokio<S> {
    type Error = io::Error;

    fn start_send(self: Pin<&mut Self>, item: Execute<C, I>) -> Result<(), Self::Error> {
        self.stream().start_send(item)
    }

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        Sink::<Execute<C, I>>::poll_ready(self.stream(), cx)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        Sink::<Execute<C, I>>::poll_flush(self.stream(), cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        Sink::<Execute<C, I>>::poll_close(self.stream(), cx)
    }
}

#[cfg(feature = "qapi-qmp")]
pub struct QmpStreamTokio<S> {
    stream: Framed<S, JsonLinesCodec<QmpMessageAny>>,
}

#[cfg(feature = "qapi-qmp")]
impl<S> QmpStreamTokio<S> {
    fn stream(self: Pin<&mut Self>) -> Pin<&mut Framed<S, JsonLinesCodec<QmpMessageAny>>> {
        unsafe {
            self.map_unchecked_mut(|this| &mut this.stream)
        }
    }
}

#[cfg(feature = "qapi-qmp")]
impl<S: AsyncRead> Stream for QmpStreamTokio<S> {
    type Item = io::Result<QmpMessageAny>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        self.stream().poll_next(cx)
    }
}

#[cfg(feature = "qapi-qmp")]
impl<S: AsyncWrite, C: QmpCommand, I: serde::Serialize> Sink<Execute<C, I>> for QmpStreamTokio<S> {
    type Error = io::Error;

    fn start_send(self: Pin<&mut Self>, item: Execute<C, I>) -> Result<(), Self::Error> {
        self.stream().start_send(item)
    }

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        Sink::<Execute<C, I>>::poll_ready(self.stream(), cx)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        Sink::<Execute<C, I>>::poll_flush(self.stream(), cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        Sink::<Execute<C, I>>::poll_close(self.stream(), cx)
    }
}

#[cfg(feature = "qapi-qmp")]
impl<S> QmpStreamTokio<S> {
    pub fn new(stream: S) -> Self {
        Self {
            stream: Framed::from_parts(FramedParts::new::<()>(stream, JsonLinesCodec::<QmpMessageAny>::new())),
        }
    }

    pub async fn open_split<W>(read: S, write: W) -> io::Result<QmpStreamNegotiation<Self, QmpStreamTokio<W>>> where
        S: AsyncRead + Unpin,
    {
        use futures::StreamExt;

        let mut lines = Framed::from_parts(FramedParts::new::<()>(read, JsonLinesCodec::<QapiCapabilities>::new()));

        let capabilities = lines.next().await.ok_or_else(||
            io::Error::new(io::ErrorKind::UnexpectedEof, "QMP greeting expected")
        )??;

        let lines = lines.into_parts();
        let mut read = FramedParts::new::<()>(lines.io, JsonLinesCodec::new());
        read.read_buf = lines.read_buf;
        let stream = Framed::from_parts(read);

        let supports_oob = capabilities.capabilities().any(|c| c == QMPCapability::oob);
        let shared = Arc::new(QapiShared::new(supports_oob));
        let events = QapiEvents {
            stream: Self { stream },
            shared: shared.clone(),
        };
        let service = QapiService::new(QmpStreamTokio::new(write), shared);

        Ok(QmpStreamNegotiation {
            stream: QapiStream {
                service,
                events,
            },
            capabilities,
        })
    }
}

#[cfg(feature = "qapi-qmp")]
impl<RW: AsyncRead + AsyncWrite> QmpStreamTokio<ReadHalf<RW>> {
    pub async fn open(stream: RW) -> io::Result<QmpStreamNegotiation<Self, QmpStreamTokio<WriteHalf<RW>>>> where RW: Unpin {
        let (r, w) = split(stream);
        Self::open_split(r, w).await
    }
}

#[cfg(all(feature = "qapi-qmp", feature = "async-tokio-uds"))]
impl QmpStreamTokio<ReadHalf<tokio::net::UnixStream>> {
    pub async fn open_uds<P: AsRef<std::path::Path>>(socket_addr: P) -> io::Result<QmpStreamNegotiation<Self, QmpStreamTokio<WriteHalf<tokio::net::UnixStream>>>> {
        let socket = tokio::net::UnixStream::connect(socket_addr).await?;
        let (r, w) = split(socket);
        Self::open_split(r, w).await
    }
}
