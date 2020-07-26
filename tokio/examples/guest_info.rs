#[cfg(feature = "qga")]
mod main {
    use std::env::args;
    use std::io;
    use futures::future::{FutureExt, TryFutureExt};
    use tokio::runtime::Runtime;

    pub fn main() {
        ::env_logger::init();

        let mut rt = Runtime::new().unwrap();

        let socket_addr = args().nth(1).expect("argument: QEMU Guest Agent socket path");

        rt.block_on(async {
            let socket = tokio::net::UnixStream::connect(socket_addr).await?;
            let stream = tokio_qapi::QgaStream::open(socket);
            let stream = stream.spawn();
            let sync_value = &stream as *const _ as usize as u32;
            stream.guest_sync(sync_value).await?;

            let info = stream.execute(tokio_qapi::qga::guest_info { }).await??;
            println!("Guest Agent version: {}", info.version);

            Ok(())
        }.map_err(|err: io::Error| panic!("Failed with {:?}", err)).boxed()).unwrap();
    }
}

#[cfg(not(feature = "qga"))]
mod main {
    pub fn main() { panic!("requires feature qga") }
}

fn main() { main::main() }
