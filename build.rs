use std::io;
use tonic_build;

fn main() -> io::Result<()> {
    tonic_build::compile_protos("replica.proto")?;

    Ok(())
}