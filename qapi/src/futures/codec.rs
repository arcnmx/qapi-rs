use std::io;
use std::marker::PhantomData;
use bytes::{BytesMut, BufMut};
use serde::{de::DeserializeOwned, Serialize};

pub struct JsonLinesCodec<D = ()> {
    next_index: usize,
    _decoder: PhantomData<fn() -> D>,
}

impl<D> JsonLinesCodec<D> {
    pub fn new() -> Self {
        Self {
            next_index: 0,
            _decoder: PhantomData,
        }
    }
}

impl<D: DeserializeOwned> JsonLinesCodec<D> {
    fn priv_decode(&mut self, buf: &mut BytesMut) -> Result<Option<D>, io::Error> {
        match memchr::memchr(b'\n', &buf[self.next_index..]) {
            Some(offset) => {
                let index = offset + self.next_index;
                self.next_index = 0;
                let line = buf.split_to(index + 1);
                serde_json::from_slice(&line)
                    .map_err(From::from)
                    .map(Some)
            },
            None => {
                self.next_index = buf.len();
                Ok(None)
            },
        }
    }

    fn priv_decode_eof(&mut self, buf: &mut BytesMut) -> Result<Option<D>, io::Error> {
        if buf.is_empty() {
            Ok(None)
        } else {
            serde_json::from_slice(buf)
                .map_err(From::from)
                .map(Some)
        }
    }
}

#[cfg(feature = "tokio-util")]
impl<D: DeserializeOwned> tokio_util::codec::Decoder for JsonLinesCodec<D> {
    type Item = D;
    type Error = io::Error;

    fn decode(&mut self, buf: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        self.priv_decode(buf)
    }

    fn decode_eof(&mut self, buf: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        self.priv_decode_eof(buf)
    }
}

fn encode<S: Serialize>(item: S, bytes: &mut BytesMut) -> Result<(), io::Error> {
    serde_json::to_writer(bytes.writer(), &item)?;
    bytes.put_u8(b'\n');
    Ok(())
}

#[cfg(feature = "tokio-util")]
impl<T, S: Serialize> tokio_util::codec::Encoder<S> for JsonLinesCodec<T> {
    type Error = io::Error;

    fn encode(&mut self, item: S, bytes: &mut BytesMut) -> Result<(), Self::Error> {
        encode(item, bytes)
    }
}
