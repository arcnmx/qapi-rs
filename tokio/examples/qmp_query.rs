#![feature(async_await, await_macro)]

#[cfg(feature = "qmp")]
mod main {
    use std::env::args;
    use std::io;
    use futures::compat::Future01CompatExt;
    use futures::future::{FutureExt, TryFutureExt, abortable};

    pub fn main() {
        ::env_logger::init();

        let socket_addr = args().nth(1).expect("argument: QMP socket path");

        tokio::run(async {
            let socket = await!(tokio_uds::UnixStream::connect(socket_addr).compat())?;
            let (caps, stream, events) = await!(tokio_qapi::QapiStream::open_tokio(socket))?;
            println!("{:#?}", caps);

            let (events, abort) = abortable(events.spin());
            tokio::spawn(events.map_err(drop).boxed().compat());

            let status = await!(stream.execute(tokio_qapi::qmp::query_status { }))??;
            println!("VCPU status: {:#?}", status);

            abort.abort();

            Ok(())
        }.map_err(|err: io::Error| panic!("Failed with {:?}", err)).boxed().compat());
    }
}

#[cfg(not(feature = "qmp"))]
mod main {
    pub fn main() { panic!("requires feature qmp") }
}

fn main() { main::main() }
