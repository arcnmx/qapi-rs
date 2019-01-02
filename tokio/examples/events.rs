#![feature(async_await, await_macro)]

#[cfg(feature = "qmp")]
mod main {
    use std::env::args;
    use std::io;
    use futures::compat::Future01CompatExt;
    use futures::future::{FutureExt, TryFutureExt};
    use futures::stream::StreamExt;

    pub fn main() {
        ::env_logger::init();

        let socket_addr = args().nth(1).expect("argument: QMP socket path");

        tokio::run(async {
            let socket = await!(tokio_uds::UnixStream::connect(socket_addr).compat())?;
            let (caps, stream, events) = await!(tokio_qapi::QapiStream::open_tokio(socket))?;
            println!("{:#?}", caps);
            let _stream = await!(stream)?;

            let mut events = events.into_stream().boxed();
            while let Some(event) = await!(events.next()) {
                println!("Got event {:#?}", event?);
            }

            Ok(())
        }.map_err(|err: io::Error| panic!("Failed with {:?}", err)).boxed().compat());
    }
}

#[cfg(not(feature = "qmp"))]
mod main {
    pub fn main() { panic!("requires feature qmp") }
}

fn main() { main::main() }
