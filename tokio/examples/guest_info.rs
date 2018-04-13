extern crate tokio_qapi;
extern crate tokio_uds;
extern crate tokio;
extern crate futures;
extern crate env_logger;

#[cfg(feature = "qga")]
mod main {
    use std::env::args;
    use tokio_uds::UnixStream;
    use tokio::prelude::*;
    use tokio::run;
    use tokio_qapi::{self, qga};

    pub fn main() {
        ::env_logger::init();

        let socket_addr = args().nth(1).expect("argument: QEMU Guest Agent socket path");

        let stream = UnixStream::connect(socket_addr).expect("failed to connect to socket");
        let stream = tokio_qapi::stream(stream);
        run(tokio_qapi::qga_handshake(stream)
            .and_then(|stream| stream.execute(qga::guest_info { }))
            .and_then(|(info, _stream)| info.map_err(From::from))
            .map(|info| println!("Guest Agent version: {}", info.version))
            .map_err(|e| panic!("Failed with {:?}", e))
        );
    }
}

#[cfg(not(feature = "qga"))]
mod main {
    pub fn main() { panic!("requires feature qga") }
}

fn main() { main::main() }
