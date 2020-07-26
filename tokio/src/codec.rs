use std::{io, str};
use std::marker::PhantomData;
use tokio_util::codec::{Encoder, Decoder, LinesCodec as Codec, LinesCodecError};
use bytes::{BytesMut, BufMut};
use serde::{de::DeserializeOwned, Serialize};
use log::trace;

pub struct JsonLinesCodec<D> {
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

impl<D: DeserializeOwned> Decoder for JsonLinesCodec<D> {
    type Item = D;
    type Error = io::Error;

    fn decode(&mut self, buf: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        // TODO: memchr
        let offset = buf[self.next_index..]
            .iter()
            .position(|b| *b == b'\n');

        match offset {
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

    fn decode_eof(&mut self, buf: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        if buf.is_empty() {
            Ok(None)
        } else {
            serde_json::from_slice(buf)
                .map_err(From::from)
                .map(Some)
        }
    }
}

struct BytesWriter<'a> {
    bytes: &'a mut BytesMut,
}

impl<'a> io::Write for BytesWriter<'a> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.bytes.put(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<T, S: Serialize> Encoder<S> for JsonLinesCodec<T> {
    type Error = io::Error;

    fn encode(&mut self, item: S, bytes: &mut BytesMut) -> Result<(), Self::Error> {
        serde_json::to_writer(BytesWriter { bytes }, &item)?;
        bytes.put_u8(b'\n');
        Ok(())
    }
}
