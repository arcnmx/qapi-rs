#[cfg(feature = "qmp")]
mod main {
    use std::thread::sleep;
    use std::time::Duration;
    use std::env::args;
    use std::os::unix::net::UnixStream;
    use qapi::{qmp, Qmp};

    pub fn main() {
        ::env_logger::init();

        let socket_addr = args().nth(1).expect("argument: QMP socket path");
        let stream = UnixStream::connect(socket_addr).expect("failed to connect to socket");

        let mut qmp = Qmp::from_stream(&stream);

        let info = qmp.handshake().expect("handshake failed");
        println!("QMP info: {:#?}", info);

        let status = qmp.execute(&qmp::query_status { }).unwrap().unwrap();
        println!("VCPU status: {:#?}", status);

        loop {
            qmp.nop().unwrap();
            for event in qmp.events() {
                println!("Got event: {:#?}", event);
            }

            sleep(Duration::from_secs(1));
        }
    }
}

#[cfg(not(feature = "qmp"))]
mod main {
    pub fn main() { panic!("requires feature qmp") }
}

fn main() { main::main() }
