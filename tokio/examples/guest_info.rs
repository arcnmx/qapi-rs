#![feature(async_await, await_macro)]

#[cfg(feature = "qga")]
mod main {
    use std::env::args;
    use std::io;
    use futures::compat::Future01CompatExt;
    use futures::future::{FutureExt, TryFutureExt, abortable};

    pub fn main() {
        ::env_logger::init();

        let socket_addr = args().nth(1).expect("argument: QEMU Guest Agent socket path");

        tokio::run(async {
            let socket = await!(tokio_uds::UnixStream::connect(socket_addr).compat())?;
            let (stream, events) = await!(tokio_qapi::QapiStream::open_tokio_qga(socket))?;

            let (events, abort) = abortable(events);
            tokio::spawn(events.map_err(drop).boxed().compat());

            let info = await!(stream.execute(tokio_qapi::qga::guest_info { }))??;
            println!("Guest Agent version: {}", info.version);

            abort.abort();

            Ok(())
        }.map_err(|err: io::Error| panic!("Failed with {:?}", err)).boxed().compat());
    }
}

#[cfg(not(feature = "qga"))]
mod main {
    pub fn main() { panic!("requires feature qga") }
}

fn main() { main::main() }
