//! Fixtures crudos → salida canónica esperada.
//!
//! Cada `tests/fixtures/<carpeta>/<nombre>.json` tiene al lado `<nombre>.esperado` con la
//! salida de [`render::fixture`]. `BENDER_BLESS=1 cargo test -p bender-venue-binance
//! --test fixtures` reescribe los `.esperado` (revisar el diff antes de commitear).
//! `documentados/` sale de los ejemplos de la documentación de Binance; `reales/`, de
//! capturas con `bender-recorder capture`.

use std::path::Path;

use bender_venue_binance::render;

fn check_dir(dir: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    let mut files: Vec<_> = entries
        .map(|e| e.expect("entrada").path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect();
    files.sort();
    let bless = std::env::var_os("BENDER_BLESS").is_some();
    let mut checked = 0;
    for path in files {
        let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
        let raw = std::fs::read(&path).expect("leer fixture");
        let snapshot = if stem.contains("depth") && !stem.contains("snapshot") {
            let market = stem.split('_').next().unwrap();
            let suffix = stem.strip_prefix(&format!("{market}_depth")).unwrap_or("");
            std::fs::read(dir.join(format!("{market}_depth_snapshot{suffix}.json"))).ok()
        } else {
            None
        };
        let got = render::fixture(&stem, &raw, snapshot.as_deref())
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let expected_path = path.with_extension("esperado");
        if bless {
            std::fs::write(&expected_path, &got).expect("escribir .esperado");
        }
        let expected = std::fs::read_to_string(&expected_path)
            .unwrap_or_else(|_| panic!("falta {}", expected_path.display()));
        assert_eq!(got, expected, "{}", path.display());
        checked += 1;
    }
    checked
}

#[test]
fn documented_fixtures() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/documentados");
    assert!(check_dir(&dir) >= 12);
}

#[test]
fn real_fixtures() {
    // Las capturas reales se agregan con `bender-recorder capture`.
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/reales");
    check_dir(&dir);
}
