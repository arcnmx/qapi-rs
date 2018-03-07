#[macro_use]
extern crate log;
extern crate qapi_spec as spec;
extern crate serde_json;
extern crate serde;

#[cfg(feature = "qapi-qmp")]
pub extern crate qapi_qmp as qmp;

#[cfg(feature = "qapi-qga")]
pub extern crate qapi_qga as qga;

pub use spec::{Any, Empty, Command, Event, Error, Timestamp};

#[cfg(any(feature = "qapi-qmp", feature = "qapi-qga"))]
mod qapi {
    use serde_json;
    use serde::Deserialize;
    use std::io::{self, BufRead, Write};
    use {spec, Command};

    pub struct Qapi<S> {
        pub stream: S,
        pub buffer: String,
    }

    impl<S> Qapi<S> {
        pub fn new(s: S) -> Self {
            Qapi {
                stream: s,
                buffer: String::new(),
            }
        }
    }

    impl<S: BufRead> Qapi<S> {
        pub fn decode_line<'de, D: Deserialize<'de>>(&'de mut self) -> io::Result<Option<D>> {
            self.buffer.clear();
            let line = self.stream.read_line(&mut self.buffer)?;
            trace!("<- {}", self.buffer);

            if line == 0 {
                Ok(None)
            } else {
                serde_json::from_str(&self.buffer).map(Some).map_err(From::from)
            }
        }
    }

    impl<S: Write> Qapi<S> {
        pub fn write_command<C: Command>(&mut self, command: &C) -> io::Result<()> {
            {
                let mut ser = serde_json::Serializer::new(&mut self.stream);
                spec::serde_command::serialize(command, &mut ser)?;

                trace!("-> execute {}: {}", C::NAME, serde_json::to_string_pretty(command).unwrap());
            }

            self.stream.write(&[b'\n'])?;

            self.stream.flush()
        }
    }
}

mod stream {
    use std::io::{Read, Write, BufRead, Result};

    pub struct Stream<R, W> {
        r: R,
        w: W,
    }

    impl<R, W> Stream<R, W> {
        pub fn new(r: R, w: W) -> Self {
            Stream {
                r: r,
                w: w,
            }
        }

        pub fn into_inner(self) -> (R, W) {
            (self.r, self.w)
        }

        pub fn get_ref_read(&self) -> &R { &self.r }
        pub fn get_mut_read(&mut self) -> &mut R { &mut self.r }
        pub fn get_ref_write(&self) -> &W { &self.w }
        pub fn get_mut_write(&mut self) -> &mut W { &mut self.w }
    }

    impl<R: Read, W> Read for Stream<R, W> {
        fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
            self.r.read(buf)
        }
    }

    impl<R: BufRead, W> BufRead for Stream<R, W> {
        fn fill_buf(&mut self) -> Result<&[u8]> {
            self.r.fill_buf()
        }

        fn consume(&mut self, amt: usize) {
            self.r.consume(amt)
        }
    }

    impl<R, W: Write> Write for Stream<R, W> {
        fn write(&mut self, buf: &[u8]) -> Result<usize> {
            self.w.write(buf)
        }

        fn flush(&mut self) -> Result<()> {
            self.w.flush()
        }
    }
}

pub use stream::Stream;

#[cfg(feature = "qapi-qmp")]
mod qmp_impl {
    use std::io::{self, BufRead, Read, Write, BufReader};
    use std::vec::Drain;
    use spec::{Error, ResponseEvent};
    use qapi::Qapi;
    use qmp::{QMP, QapiCapabilities, Event, qmp_capabilities, query_version};
    use {Command, Stream};

    pub struct Qmp<S> {
        inner: Qapi<S>,
        event_queue: Vec<Event>,
    }

    impl<'a, S: 'a> Qmp<Stream<BufReader<&'a S>, &'a S>> where &'a S: Read + Write {
        pub fn from_stream(s: &'a S) -> Self {
            Self::new(Stream::new(BufReader::new(s), s))
        }
    }

    impl<S> Qmp<S> {
        pub fn new(stream: S) -> Self {
            Qmp {
                inner: Qapi::new(stream),
                event_queue: Default::default(),
            }
        }

        pub fn into_inner(self) -> S {
            self.inner.stream
        }

        pub fn inner(&self) -> &S {
            &self.inner.stream
        }

        pub fn inner_mut(&mut self) -> &mut S {
            &mut self.inner.stream
        }

        pub fn events(&mut self) -> Drain<Event> {
            self.event_queue.drain(..)
        }
    }

    impl<S: BufRead> Qmp<S> {
        pub fn read_capabilities(&mut self) -> io::Result<QMP> {
            self.inner.decode_line().map(|v: Option<QapiCapabilities>|
                v.expect("unexpected eof").QMP
            )
        }

        pub fn read_response<C: Command>(&mut self) -> io::Result<Result<C::Ok, Error>> {
            loop {
                match self.inner.decode_line()? {
                    None => return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "expected command response")),
                    Some(ResponseEvent::Ok { return_ }) => return Ok(Ok(return_)),
                    Some(ResponseEvent::Err(e)) => return Ok(Err(e)),
                    Some(ResponseEvent::Event(e)) => self.event_queue.push(e),
                }
            }
        }
    }

    impl<S: BufRead + Write> Qmp<S> {
        pub fn write_command<C: Command>(&mut self, command: &C) -> io::Result<()> {
            self.inner.write_command(command)
        }

        pub fn execute<C: Command>(&mut self, command: &C) -> io::Result<Result<C::Ok, Error>> {
            self.write_command(command)?;
            self.read_response::<C>()
        }

        pub fn handshake(&mut self) -> io::Result<QMP> {
            let caps = self.read_capabilities()?;
            self.execute(&qmp_capabilities { })
                .and_then(|v| v.map_err(From::from))
                .map(|_| caps)
        }

        /// Can be used to poll the socket for pending events
        pub fn nop(&mut self) -> io::Result<()> {
            self.execute(&query_version { })
                .and_then(|v| v.map_err(From::from))
                .map(drop)
        }
    }
}

#[cfg(feature = "qapi-qga")]
mod qga_impl {
    use std::io::{self, BufRead, Read, Write, BufReader};
    use spec::{Error, Response};
    use qapi::Qapi;
    use qga::guest_sync;
    use {Command, Stream};

    pub struct Qga<S> {
        inner: Qapi<S>,
    }

    impl<'a, S: 'a> Qga<Stream<BufReader<&'a S>, &'a S>> where &'a S: Read + Write {
        pub fn from_stream(s: &'a S) -> Self {
            Self::new(Stream::new(BufReader::new(s), s))
        }
    }

    impl<S> Qga<S> {
        pub fn new(stream: S) -> Self {
            Qga {
                inner: Qapi::new(stream),
            }
        }

        pub fn into_inner(self) -> S {
            self.inner.stream
        }

        pub fn inner(&self) -> &S {
            &self.inner.stream
        }

        pub fn inner_mut(&mut self) -> &mut S {
            &mut self.inner.stream
        }
    }

    impl<S: BufRead> Qga<S> {
        pub fn read_response<C: Command>(&mut self) -> io::Result<Result<C::Ok, Error>> {
            loop {
                match self.inner.decode_line()? {
                    None => return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "expected command response")),
                    Some(Response::Ok { return_ }) => return Ok(Ok(return_)),
                    Some(Response::Err(e)) => return Ok(Err(e)),
                }
            }
        }
    }

    impl<S: BufRead + Write> Qga<S> {
        pub fn write_command<C: Command>(&mut self, command: &C) -> io::Result<()> {
            self.inner.write_command(command)
        }

        pub fn execute<C: Command>(&mut self, command: &C) -> io::Result<Result<C::Ok, Error>> {
            self.write_command(command)?;
            self.read_response::<C>()
        }

        pub fn handshake(&mut self) -> io::Result<()> {
            let sync = guest_sync {
                id: self as *mut _ as usize as _, // TODO: need better source of random id than a pointer...
            };

            match self.execute(&sync)? {
                Ok(r) if r == sync.id => Ok(()),
                Ok(..) => Err(io::Error::new(io::ErrorKind::InvalidData, "guest-sync handshake failed")),
                Err(e) => Err(e.into()),
            }
        }
    }
}

#[cfg(feature = "qapi-qmp")]
pub use qmp_impl::*;

#[cfg(feature = "qapi-qga")]
pub use qga_impl::*;
