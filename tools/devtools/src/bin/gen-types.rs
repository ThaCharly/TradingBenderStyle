//! Genera el código Rust desde `schema/` (prost y tonic, sin `protoc`: el esquema se
//! compila con protox). Los tipos TS los genera `buf generate`. Uso: `make gen`.
//!
//! - `bender.v1` (mensajes) → `crates/bender-types/src/gen/bender.v1.rs`
//! - `bender.example.v1` (servicio de ejemplo, cliente y servidor tonic) →
//!   `crates/bender-transport/src/gen/bender.example.v1.rs`

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use prost_types::FileDescriptorSet;

fn collect_protos(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("leer schema/") {
        let path = entry.expect("entrada del esquema").path();
        if path.is_dir() {
            collect_protos(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "proto") {
            out.push(path);
        }
    }
}

/// Sólo los archivos de `package`.
fn only_package(fds: &FileDescriptorSet, package: &str) -> FileDescriptorSet {
    FileDescriptorSet {
        file: fds
            .file
            .iter()
            .filter(|f| f.package() == package)
            .cloned()
            .collect(),
    }
}

fn rustfmt(file: &Path) -> bool {
    Command::new("rustfmt")
        .args(["--edition", "2024"])
        .arg(file)
        .status()
        .is_ok_and(|s| s.success())
}

fn main() -> ExitCode {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let schema = root.join("schema");

    let mut protos = Vec::new();
    collect_protos(&schema, &mut protos);
    protos.sort();
    let fds = match protox::compile(&protos, [&schema]) {
        Ok(fds) => fds,
        Err(err) => {
            eprintln!("error compilando el esquema: {err:?}");
            return ExitCode::FAILURE;
        }
    };

    let types_out = root.join("crates/bender-types/src/gen");
    std::fs::create_dir_all(&types_out).expect("crear bender-types/src/gen");
    let mut config = prost_build::Config::new();
    config.out_dir(&types_out);
    if let Err(err) = config.compile_fds(only_package(&fds, "bender.v1")) {
        eprintln!("error generando bender.v1: {err}");
        return ExitCode::FAILURE;
    }

    let transport_out = root.join("crates/bender-transport/src/gen");
    std::fs::create_dir_all(&transport_out).expect("crear bender-transport/src/gen");
    let services = tonic_prost_build::configure()
        .out_dir(&transport_out)
        .extern_path(".bender.v1", "::bender_types::proto")
        .compile_fds(only_package(&fds, "bender.example.v1"));
    if let Err(err) = services {
        eprintln!("error generando bender.example.v1: {err}");
        return ExitCode::FAILURE;
    }

    let generated = [
        types_out.join("bender.v1.rs"),
        transport_out.join("bender.example.v1.rs"),
    ];
    if generated.iter().all(|f| rustfmt(f)) {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
