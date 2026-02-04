fn main() -> std::io::Result<()> {
    prost_build::compile_protos(&["proto/RIFT.proto"], &["proto/"])?;
    Ok(())
}
