use std::env::args;
use std::io;

#[tokio::main]
async fn main() -> io::Result<()> {
    ::env_logger::init();

    let socket_addr = args().nth(1).expect("argument: QEMU Guest Agent socket path");

    let stream = qapi::futures::QgaStreamTokio::open_uds(socket_addr).await?;
    let (qga, handle) = stream.spawn_tokio();

    let sync_value = &qga as *const _ as usize as isize;
    qga.guest_sync(sync_value).await?;

    let info = qga.execute(qapi::qga::guest_info { }).await?;
    println!("Guest Agent version: {}", info.version);

    {
        // NOTE: this isn't necessary, but to manually ensure the stream closes...
        drop(qga); // relinquish handle on the stream
        handle.await?; // wait for event loop to exit
    }

    Ok(())
}
