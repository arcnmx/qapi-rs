#![doc(html_root_url = "https://docs.rs/qapi/0.12.0")]

#[cfg(feature = "qapi-qmp")]
pub use qapi_qmp as qmp;

#[cfg(feature = "qapi-qga")]
pub use qapi_qga as qga;

pub use qapi_spec::{Any, Dictionary, Empty, Never, Execute, ExecuteOob, Command, CommandResult, Event, Enum, Error, ErrorClass, Timestamp};

pub use self::stream::Stream;

#[cfg(feature = "qapi-qmp")]
pub use self::qmp_impl::*;

#[cfg(feature = "qapi-qga")]
pub use self::qga_impl::*;

use std::{error, fmt, io};

#[cfg(feature = "async")]
pub mod futures;

#[derive(Debug)]
pub enum ExecuteError {
    Qapi(Error),
    Io(io::Error),
}

pub type ExecuteResult<C> = Result<<C as Command>::Ok, ExecuteError>;

impl fmt::Display for ExecuteError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ExecuteError::Qapi(e) => fmt::Display::fmt(e, f),
            ExecuteError::Io(e) => fmt::Display::fmt(e, f),
        }
    }
}

impl error::Error for ExecuteError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            ExecuteError::Qapi(e) => Some(e),
            ExecuteError::Io(e) => Some(e),
        }
    }
}

impl From<io::Error> for ExecuteError {
    fn from(e: io::Error) -> Self {
        ExecuteError::Io(e)
    }
}

impl From<Error> for ExecuteError {
    fn from(e: Error) -> Self {
        ExecuteError::Qapi(e)
    }
}

impl From<ExecuteError> for io::Error {
    fn from(e: ExecuteError) -> Self {
        match e {
            ExecuteError::Qapi(e) => e.into(),
            ExecuteError::Io(e) => e,
        }
    }
}

#[cfg(any(feature = "qapi-qmp", feature = "qapi-qga"))]
mod qapi {
    use serde_json;
    use serde::{Serialize, Deserialize};
    use std::io::{self, BufRead, Write};
    use crate::{Command, Execute};
    use log::trace;

    pub struct Qapi<S> {
        pub stream: S,
        pub buffer: Vec<u8>,
    }

    impl<S> Qapi<S> {
        pub fn new(s: S) -> Self {
            Qapi {
                stream: s,
                buffer: Default::default(),
            }
        }
    }

    impl<S: BufRead> Qapi<S> {
        pub fn decode_line<'de, D: Deserialize<'de>>(&'de mut self) -> io::Result<Option<D>> {
            self.buffer.clear();
            let line = self.stream.read_until(b'\n', &mut self.buffer)?;
            let line = &self.buffer[..line];
            trace!("<- {}", String::from_utf8_lossy(line));

            if line.is_empty() {
                Ok(None)
            } else {
                serde_json::from_slice(line).map(Some).map_err(From::from)
            }
        }
    }

    impl<S: Write> Qapi<S> {
        pub fn encode_line<C: Serialize>(&mut self, command: &C) -> io::Result<()> {
            {
                let mut ser = serde_json::Serializer::new(&mut self.stream);
                command.serialize(&mut ser)?;
            }

            self.stream.write(&[b'\n'])?;

            self.stream.flush()
        }

        pub fn write_command<C: Command>(&mut self, command: &C) -> io::Result<()> {
            self.encode_line(&Execute::<&C>::from(command))?;

            trace!("-> execute {}: {}", C::NAME, serde_json::to_string_pretty(command).unwrap());

            Ok(())
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
                r,
                w,
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

#[cfg(feature = "qapi-qmp")]
mod qmp_impl {
    use std::io::{self, BufRead, Read, Write, BufReader};
    use std::vec::Drain;
    use qapi_qmp::{QMP, QapiCapabilities, QmpMessage, Event, qmp_capabilities, query_version};
    use crate::{qapi::Qapi, Stream, ExecuteResult, ExecuteError, Command};

    pub struct Qmp<S> {
        inner: Qapi<S>,
        event_queue: Vec<Event>,
    }

    impl<S: Read + Write + Clone> Qmp<Stream<BufReader<S>, S>> {
        pub fn from_stream(s: S) -> Self {
            Self::new(Stream::new(BufReader::new(s.clone()), s))
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

        pub fn read_response<C: Command>(&mut self) -> ExecuteResult<C> {
            loop {
                match self.inner.decode_line()? {
                    None => return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "expected command response").into()),
                    Some(QmpMessage::Response(res)) => return res.result().map_err(From::from),
                    Some(QmpMessage::Event(e)) => self.event_queue.push(e),
                }
            }
        }
    }

    impl<S: BufRead + Write> Qmp<S> {
        pub fn write_command<C: Command>(&mut self, command: &C) -> io::Result<()> {
            self.inner.write_command(command)
        }

        pub fn execute<C: Command>(&mut self, command: &C) -> ExecuteResult<C> {
            self.write_command(command)?;
            self.read_response::<C>()
        }

        pub fn handshake(&mut self) -> Result<QMP, ExecuteError> {
            let caps = self.read_capabilities()?;
            self.execute(&qmp_capabilities { enable: None })
                .map(|_| caps)
        }

        /// Can be used to poll the socket for pending events
        pub fn nop(&mut self) -> io::Result<()> {
            self.execute(&query_version { })
                .map_err(From::from)
                .map(drop)
        }
    }
}

#[cfg(feature = "qapi-qga")]
mod qga_impl {
    use std::io::{self, BufRead, Read, Write, BufReader};
    use qapi_qga::guest_sync;
    use qapi_spec::Response;
    use crate::{qapi::Qapi, Stream, Command, ExecuteResult, ExecuteError};

    pub struct Qga<S> {
        inner: Qapi<S>,
    }

    impl<S: Read + Write + Clone> Qga<Stream<BufReader<S>, S>> {
        pub fn from_stream(s: S) -> Self {
            Self::new(Stream::new(BufReader::new(s.clone()), s))
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
        pub fn read_response<C: Command>(&mut self) -> ExecuteResult<C> {
            loop {
                match self.inner.decode_line()?.map(|r: Response<_>| r.result()) {
                    None => return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "expected command response").into()),
                    Some(Ok(res)) => return Ok(res),
                    Some(Err(e)) => return Err(e.into()),
                }
            }
        }
    }

    impl<S: BufRead + Write> Qga<S> {
        pub fn write_command<C: Command>(&mut self, command: &C) -> io::Result<()> {
            self.inner.write_command(command)
        }

        pub fn execute<C: Command>(&mut self, command: &C) -> ExecuteResult<C> {
            self.write_command(command)?;
            self.read_response::<C>()
        }

        pub fn guest_sync(&mut self, sync_value: i32) -> Result<(), ExecuteError> {
            let id = sync_value.into();
            let sync = guest_sync {
                id,
            };

            match self.execute(&sync) {
                Ok(r) if r == sync.id => Ok(()),
                Ok(..) => Err(io::Error::new(io::ErrorKind::InvalidData, "guest-sync handshake failed").into()),
                Err(e) => Err(e.into()),
            }
        }
    }
}
