//! Verifica que los enlaces relativos de los Markdown del repo apunten a archivos existentes.

use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let root = root.canonicalize().expect("raíz del repo");
    match bender_devtools::find_broken_links(&root) {
        Ok(broken) if broken.is_empty() => ExitCode::SUCCESS,
        Ok(broken) => {
            for link in broken {
                eprintln!("enlace roto: {} -> {}", link.file.display(), link.target);
            }
            ExitCode::FAILURE
        }
        Err(err) => {
            eprintln!("error leyendo la documentación: {err}");
            ExitCode::FAILURE
        }
    }
}
