extern crate tokio_qapi;
extern crate tokio_uds;
extern crate tokio_core;
extern crate futures;
extern crate qapi;
extern crate env_logger;

#[cfg(feature = "qga")]
mod main {
    use std::env::args;
    use tokio_uds::UnixStream;
    use tokio_core::reactor::Core;
    use tokio_qapi::{self, qga};

    pub fn main() {
        ::env_logger::init();

        let socket_addr = args().nth(1).expect("argument: QEMU Guest Agent socket path");

        let mut core = Core::new().expect("failed to create core");
        let stream = UnixStream::connect(socket_addr, &core.handle()).expect("failed to connect to socket");
        let stream = tokio_qapi::stream(stream);
        let stream = core.run(tokio_qapi::qga_handshake(stream)).expect("failed to handshake");
        let info = tokio_qapi::execute(qga::guest_info { }, stream);
        let (info, _stream) = core.run(info).expect("failed to complete future");
        let info = info.expect("failed to get guest info");
        println!("Guest Agent version: {}", info.version);
    }
}

#[cfg(not(feature = "qga"))]
mod main {
    pub fn main() { panic!("requires feature qga") }
}

fn main() { main::main() }
