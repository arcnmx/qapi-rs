#![feature(async_await, await_macro)]

#[cfg(feature = "qmp")]
mod main {
    use std::env::args;
    use std::io;
    use futures::compat::Future01CompatExt;
    use futures::future::{FutureExt, TryFutureExt};

    pub fn main() {
        ::env_logger::init();

        let socket_addr = args().nth(1).expect("argument: QMP socket path");

        // TODO: Switch to run_async and spawn_async, seems buggy currently
        tokio::run(async {
            let socket = await!(tokio_uds::UnixStream::connect(socket_addr).compat())?;
            let (caps, stream, events) = await!(tokio_qapi::QapiStream::open_tokio(socket))?;
            println!("{:#?}", caps);

            tokio::spawn(events.spin().map(|r| Ok(r)).boxed().compat());

            let mut stream = await!(stream)?;
            let status = await!(stream.execute(tokio_qapi::qmp::query_status { }))?;
            println!("VCPU status: {:#?}", status);

            Ok(())
        }.map_err(|err: io::Error| panic!("Failed with {:?}", err)).boxed().compat());
    }
}

#[cfg(not(feature = "qmp"))]
mod main {
    pub fn main() { panic!("requires feature qmp") }
}

fn main() { main::main() }
