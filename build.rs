use std::{fs, path::PathBuf};

fn main() {
    protobuf_codegen::Codegen::new()
        .pure()
        .cargo_out_dir("dropbox_proto")
        .include("proto")
        .input("proto/dropbox.proto")
        .run_from_script();

    let generated =
        PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("dropbox_proto/dropbox.rs");
    let source = fs::read_to_string(&generated).expect("read generated DropBox protobuf");
    let source = source
        .lines()
        .filter(|line| !line.starts_with("#![") && !line.starts_with("//!"))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(generated, source).expect("prepare generated DropBox protobuf for module inclusion");
}
