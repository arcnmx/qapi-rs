#[cfg(feature = "qga")]
mod main {
    use std::os::unix::net::UnixStream;
    use std::env::args;
    use qapi::{qga, Qga};

    pub fn main() {
        ::env_logger::init();

        let socket_addr = args().nth(1).expect("argument: QEMU Guest Agent socket path");
        let stream = UnixStream::connect(socket_addr).expect("failed to connect to socket");

        let mut qga = Qga::from_stream(&stream);

        qga.handshake().expect("handshake failed");
        let info = qga.execute(&qga::guest_info { }).unwrap().unwrap();
        println!("Guest Agent version: {}", info.version);
    }
}

#[cfg(not(feature = "qga"))]
mod main {
    pub fn main() { panic!("requires feature qga") }
}

fn main() { main::main() }
