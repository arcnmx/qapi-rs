extern crate tokio_qapi;
extern crate tokio_uds;
extern crate tokio;
extern crate futures;
extern crate env_logger;

#[cfg(feature = "qmp")]
mod main {
    use std::env::args;
    use tokio_uds::UnixStream;
    use tokio::prelude::*;
    use tokio::run;
    use tokio_qapi::{self, qmp};

    pub fn main() {
        ::env_logger::init();

        let socket_addr = args().nth(1).expect("argument: QMP socket path");

        let stream = UnixStream::connect(socket_addr).expect("failed to connect to socket");
        let stream = tokio_qapi::stream(stream);

        run(tokio_qapi::qmp_handshake(stream)
            .and_then(|(caps, stream)| {
                println!("{:#?}", caps);
                stream.execute(qmp::query_status { })
            }).and_then(|(status, _stream)| status.map_err(From::from))
            .map(|status| println!("VCPU status: {:#?}", status))
            .map_err(|e| panic!("Failed with {:?}", e))
        );
    }
}

#[cfg(not(feature = "qmp"))]
mod main {
    pub fn main() { panic!("requires feature qmp") }
}

fn main() { main::main() }
