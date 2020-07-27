use std::env::args;
use std::io;
use futures::StreamExt;

#[tokio::main]
async fn main() -> io::Result<()> {
    ::env_logger::init();

    let socket_addr = args().nth(1).expect("argument: QMP socket path");

    let stream = qapi::futures::QmpStreamTokio::open_uds(socket_addr).await?;
    println!("{:#?}", stream.capabilities);
    let stream = stream.negotiate().await?;
    let (_, mut events) = stream.into_parts();

    while let Some(event) = events.next().await {
        println!("Got event {:#?}", event?);
    }

    Ok(())
}
