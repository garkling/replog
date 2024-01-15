use std::io;
use tonic_build;

fn main() -> io::Result<()> {
    tonic_build::configure()
        .compile(&[
            "proto/replica.proto",
            "proto/joinreq.proto",
            "proto/syncreq.proto"
        ], &["proto"])?;

    Ok(())
}
