//! Verifica el esquema Protobuf: sin floats y con presencia explícita (ver `schema.rs`).

use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schema");
    match bender_devtools::schema::check_schema_dir(&dir) {
        Ok(issues) if issues.is_empty() => ExitCode::SUCCESS,
        Ok(issues) => {
            for issue in issues {
                eprintln!(
                    "schema/{}:{}: {}",
                    issue.file.display(),
                    issue.line,
                    issue.message
                );
            }
            ExitCode::FAILURE
        }
        Err(err) => {
            eprintln!("error leyendo el esquema: {err}");
            ExitCode::FAILURE
        }
    }
}
