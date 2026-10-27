//! Herramientas de desarrollo del repositorio.

use std::path::{Path, PathBuf};

/// Directorios que no son documentación del proyecto.
const SKIP_DIRS: &[&str] = &[".git", "node_modules", "target", "dist"];

/// Un enlace relativo de Markdown que apunta a un archivo inexistente.
#[derive(Debug, PartialEq, Eq)]
pub struct BrokenLink {
    pub file: PathBuf,
    pub target: String,
}

/// Extrae los destinos de los enlaces `[texto](destino)` de un texto Markdown.
pub fn link_targets(markdown: &str) -> Vec<&str> {
    let mut targets = Vec::new();
    let mut rest = markdown;
    while let Some(start) = rest.find("](") {
        let after = &rest[start + 2..];
        match after.find(')') {
            Some(end) => {
                let target = &after[..end];
                if !target.is_empty() && !target.contains(char::is_whitespace) {
                    targets.push(target);
                }
                rest = &after[end..];
            }
            None => break,
        }
    }
    targets
}

fn is_external(target: &str) -> bool {
    target.starts_with("http://")
        || target.starts_with("https://")
        || target.starts_with("mailto:")
        || target.starts_with('#')
}

/// Busca enlaces relativos rotos en todos los `.md` bajo `root`.
pub fn find_broken_links(root: &Path) -> std::io::Result<Vec<BrokenLink>> {
    let mut files = Vec::new();
    collect_markdown(root, &mut files)?;
    files.sort();

    let mut broken = Vec::new();
    for file in files {
        let text = std::fs::read_to_string(&file)?;
        let dir = file.parent().unwrap_or(root);
        for target in link_targets(&text) {
            if is_external(target) {
                continue;
            }
            let path = target.split('#').next().unwrap_or_default();
            if !path.is_empty() && !dir.join(path).exists() {
                broken.push(BrokenLink {
                    file: file.strip_prefix(root).unwrap_or(&file).to_path_buf(),
                    target: target.to_string(),
                });
            }
        }
    }
    Ok(broken)
}

fn collect_markdown(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        if path.is_dir() {
            if !SKIP_DIRS.contains(&name) {
                collect_markdown(&path, out)?;
            }
        } else if name.ends_with(".md") {
            out.push(path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_link_targets() {
        let md = "ver [a](docs/a.md) y [b](https://x.y) y [c](b.md#seccion)";
        assert_eq!(
            link_targets(md),
            vec!["docs/a.md", "https://x.y", "b.md#seccion"]
        );
    }

    #[test]
    fn detects_broken_and_accepts_valid_links() {
        let root = std::env::temp_dir().join(format!("devtools-test-{}", std::process::id()));
        std::fs::create_dir_all(root.join("docs")).unwrap();
        std::fs::write(root.join("docs/ok.md"), "# ok").unwrap();
        std::fs::write(
            root.join("README.md"),
            "[ok](docs/ok.md) [ancla](docs/ok.md#x) [roto](docs/no.md) [web](https://a.b)",
        )
        .unwrap();

        let broken = find_broken_links(&root).unwrap();
        std::fs::remove_dir_all(&root).unwrap();

        assert_eq!(
            broken,
            vec![BrokenLink {
                file: PathBuf::from("README.md"),
                target: "docs/no.md".into()
            }]
        );
    }
}
