use std::env::args;
use std::io;

#[tokio::main]
pub async fn main() -> io::Result<()> {
    ::env_logger::init();

    let socket_addr = args().nth(1).expect("argument: QMP socket path");

    let stream = qapi::futures::QmpStreamTokio::open_uds(socket_addr).await?;
    println!("{:#?}", stream.capabilities);
    let stream = stream.negotiate().await?;
    let (qmp, handle) = stream.spawn_tokio();

    let status = qmp.execute(qapi::qmp::query_status { }).await?;
    println!("VCPU status: {:#?}", status);

    {
        // NOTE: this isn't necessary, but to manually ensure the stream closes...
        drop(qmp); // relinquish handle on the stream
        handle.await?; // wait for event loop to exit
    }

    Ok(())
}
