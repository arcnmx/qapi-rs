#[cfg(feature = "qmp")]
mod main {
    use std::env::args;
    use std::io;
    use futures::future::{FutureExt, TryFutureExt};
    use tokio::runtime::Runtime;

    pub fn main() {
        ::env_logger::init();

        let mut rt = Runtime::new().unwrap();

        let socket_addr = args().nth(1).expect("argument: QMP socket path");

        rt.block_on(async {
            let socket = tokio::net::UnixStream::connect(socket_addr).await?;
            let stream = tokio_qapi::QmpStream::open(socket).await?;
            println!("{:#?}", stream.capabilities);
            let stream = stream.negotiate().await?;
            let stream = stream.spawn();

            let status = stream.execute(tokio_qapi::qmp::query_status { }).await??;
            println!("VCPU status: {:#?}", status);

            Ok(())
        }.map_err(|err: io::Error| panic!("Failed with {:?}", err)).boxed())
        .unwrap();
    }
}

#[cfg(not(feature = "qmp"))]
mod main {
    pub fn main() { panic!("requires feature qmp") }
}

fn main() { main::main() }
