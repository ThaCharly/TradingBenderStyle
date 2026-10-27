//! `cargo run -p arch-tests`: verifica las reglas de `rules.toml` sobre el workspace real.

use std::path::Path;
use std::process::{Command, ExitCode};

use arch_tests::{Graph, Rules, check_clock, check_graph};

fn main() -> ExitCode {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = here.join("../..");

    let rules: Rules = match std::fs::read_to_string(here.join("rules.toml"))
        .map_err(|e| e.to_string())
        .and_then(|t| toml::from_str(&t).map_err(|e| e.to_string()))
    {
        Ok(rules) => rules,
        Err(err) => {
            eprintln!("rules.toml: {err}");
            return ExitCode::FAILURE;
        }
    };

    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let output = Command::new(cargo)
        .args([
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--manifest-path",
        ])
        .arg(root.join("Cargo.toml"))
        .output()
        .expect("cargo metadata");
    if !output.status.success() {
        eprintln!("{}", String::from_utf8_lossy(&output.stderr));
        return ExitCode::FAILURE;
    }
    let graph = match Graph::from_metadata(&String::from_utf8_lossy(&output.stdout)) {
        Ok(graph) => graph,
        Err(err) => {
            eprintln!("cargo metadata: {err}");
            return ExitCode::FAILURE;
        }
    };

    let mut violations = check_graph(&graph, &rules);
    violations.extend(check_clock(&graph, &rules.clock));
    if violations.is_empty() {
        println!(
            "arch-tests: {} crates, {} reglas de grafo y la de reloj: OK",
            graph.packages.len(),
            rules.forbidden.len() + rules.feature_restricted.len()
        );
        return ExitCode::SUCCESS;
    }
    for v in &violations {
        eprintln!("[{}] {}", v.rule, v.message);
    }
    ExitCode::FAILURE
}
