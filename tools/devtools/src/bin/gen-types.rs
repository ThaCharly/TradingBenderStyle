//! Genera los tipos Rust de `bender-types` desde `schema/` (prost, sin `protoc`: el
//! esquema se compila con protox). Los tipos TS los genera `buf generate`.
//! Uso: `make gen`.

use std::path::Path;
use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let schema = root.join("schema");
    let out = root.join("crates/bender-types/src/gen");

    let mut protos = Vec::new();
    for entry in std::fs::read_dir(schema.join("bender/v1")).expect("schema/bender/v1") {
        let path = entry.expect("entrada del esquema").path();
        if path.extension().is_some_and(|ext| ext == "proto") {
            protos.push(path);
        }
    }
    protos.sort();

    let fds = match protox::compile(&protos, [&schema]) {
        Ok(fds) => fds,
        Err(err) => {
            eprintln!("error compilando el esquema: {err:?}");
            return ExitCode::FAILURE;
        }
    };

    std::fs::create_dir_all(&out).expect("crear src/gen");
    let mut config = prost_build::Config::new();
    config.out_dir(&out);
    if let Err(err) = config.compile_fds(fds) {
        eprintln!("error generando Rust: {err}");
        return ExitCode::FAILURE;
    }

    let generated = out.join("bender.v1.rs");
    let status = Command::new("rustfmt")
        .args(["--edition", "2024"])
        .arg(&generated)
        .status()
        .expect("rustfmt");
    if !status.success() {
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
