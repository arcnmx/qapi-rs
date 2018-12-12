use std::{io, str};
use tokio_io::codec::{Encoder, Decoder};
use bytes::{BytesMut, BufMut};
use log::trace;

#[derive(Default)]
pub struct LineCodec;

impl Decoder for LineCodec {
    type Item = BytesMut;
    type Error = io::Error;

    fn decode(&mut self, buf: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        trace!("LineCodec::decode(): {}", str::from_utf8(buf).unwrap_or("utf8 decode failed"));
        match buf.iter().position(|&b| b == b'\n') {
            Some(i) => {
                let line = buf.split_to(i + 1);
                Ok(Some(line))
            },
            None => Ok(None),
        }
    }

    fn decode_eof(&mut self, buf: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        if buf.len() == 0 {
            Ok(None)
        } else {
            let amt = buf.len();
            let line = buf.split_to(amt);
            Ok(Some(line))
        }
    }
}

impl Encoder for LineCodec {
    type Item = Box<[u8]>;
    type Error = io::Error;

    fn encode(&mut self, item: Self::Item, into: &mut BytesMut) -> Result<(), Self::Error> {
        into.reserve(item.len());
        into.put(&item[..]);

        Ok(())
    }
}

/* revisit...
use std::marker::PhantomData;
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json;
pub struct JsonCodec<C> {
    _marker: PhantomData<fn(C) -> C>,
    lines: LineCodec,
}

impl<C> Default for JsonCodec<C> {
    fn default() -> Self {
        JsonCodec {
            _marker: Default::default(),
            lines: Default::default(),
        }
    }
}

impl<C: DeserializeOwned> Decoder for JsonCodec<C> {
    type Item = C;
    type Error = io::Error;

    fn decode(&mut self, buf: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        self.lines.decode(buf).and_then(|line| match line {
            Some(line) => serde_json::from_slice(&line).map_err(io::Error::from).map(Some),
            None => Ok(None),
        })
    }

    fn decode_eof(&mut self, buf: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        self.lines.decode_eof(buf).and_then(|line| match line {
            Some(line) => serde_json::from_slice(&line).map_err(io::Error::from).map(Some),
            None => Ok(None),
        })
    }
}

impl<C: Serialize> Encoder for JsonCodec<C> {
    type Item = C;
    type Error = io::Error;

    fn encode(&mut self, item: Self::Item, into: &mut BytesMut) -> Result<(), Self::Error> {
        serde_json::to_writer(into.writer(), &item)?;

        into.reserve(1);
        into.put_u8(b'\n');

        Ok(())
    }
}

pub struct Codec<D, E> {
    decoder: D,
    encoder: E,
}

impl<D, E> Codec<D, E> {
    pub fn new(decoder: D, encoder: E) -> Self {
        Codec {
            decoder: decoder,
            encoder: encoder,
        }
    }
}

impl<D, E: Encoder> Encoder for Codec<D, E> {
    type Item = E::Item;
    type Error = E::Error;

    fn encode(&mut self, item: Self::Item, into: &mut BytesMut) -> Result<(), Self::Error> {
        self.encoder.encode(item, into)
    }
}

impl<D: Decoder, E> Decoder for Codec<D, E> {
    type Item = D::Item;
    type Error = D::Error;

    fn decode(&mut self, buf: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        self.decoder.decode(buf)
    }

    fn decode_eof(&mut self, buf: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        self.decoder.decode_eof(buf)
    }
}
*/
