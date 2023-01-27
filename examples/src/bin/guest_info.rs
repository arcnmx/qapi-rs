#[cfg(unix)]
use std::os::unix::net::UnixStream;
use {
    qapi::{qga, Qga},
    std::env::args,
};

pub fn main() {
    ::env_logger::init();

    let socket_addr = args().nth(1).expect("argument: QEMU Guest Agent socket path");
    #[cfg(unix)]
    let stream = UnixStream::connect(socket_addr).expect("failed to connect to socket");
    #[cfg(not(unix))]
    let stream = std::net::TcpStream::connect(socket_addr).expect("failed to connect to socket");

    let mut qga = Qga::from_stream(&stream);

    let sync_value = &stream as *const _ as usize as i32;
    qga.guest_sync(sync_value).expect("handshake failed");

    let info = qga.execute(&qga::guest_info {}).unwrap();
    println!("Guest Agent version: {}", info.version);
}
