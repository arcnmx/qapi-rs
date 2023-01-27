#[cfg(unix)]
use std::os::unix::net::UnixStream;
use {
    qapi::{qmp, Qmp},
    std::{env::args, thread::sleep, time::Duration},
};

pub fn main() {
    ::env_logger::init();

    let socket_addr = args().nth(1).expect("argument: QMP socket path");
    #[cfg(unix)]
    let stream = UnixStream::connect(socket_addr).expect("failed to connect to socket");
    #[cfg(not(unix))]
    let stream = std::net::TcpStream::connect(socket_addr).expect("failed to connect to socket");

    let mut qmp = Qmp::from_stream(&stream);

    let info = qmp.handshake().expect("handshake failed");
    println!("QMP info: {:#?}", info);

    let status = qmp.execute(&qmp::query_status {}).unwrap();
    println!("VCPU status: {:#?}", status);

    loop {
        qmp.nop().unwrap();
        for event in qmp.events() {
            println!("Got event: {:#?}", event);
        }

        sleep(Duration::from_secs(1));
    }
}
