//! Tests de arquitectura (#20): reglas sobre el grafo de dependencias del workspace
//! (obtenido con `cargo metadata`) y sobre los `clippy.toml` del Pilar 2. Las reglas se
//! declaran en `rules.toml`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// Reglas de `rules.toml`.
#[derive(Debug, Deserialize)]
pub struct Rules {
    pub clock: ClockRule,
    #[serde(default)]
    pub forbidden: Vec<ForbiddenRule>,
    #[serde(default)]
    pub feature_restricted: Vec<FeatureRule>,
}

/// Crates que no pueden leer el reloj y métodos que su `clippy.toml` tiene que prohibir.
#[derive(Debug, Deserialize)]
pub struct ClockRule {
    pub crates: Vec<String>,
    pub methods: Vec<String>,
}

/// Ningún crate de `from` depende de uno de `to` (directa o, si `transitive`,
/// transitivamente).
#[derive(Debug, Deserialize)]
pub struct ForbiddenRule {
    pub id: String,
    pub description: String,
    pub from: Vec<String>,
    pub to: Vec<String>,
    pub transitive: bool,
}

/// Sólo los crates de `allowed` pueden depender de uno de `crates` con `feature` activa.
#[derive(Debug, Deserialize)]
pub struct FeatureRule {
    pub id: String,
    pub description: String,
    pub crates: Vec<String>,
    pub feature: String,
    pub allowed: Vec<String>,
}

/// Tipo de una dependencia declarada.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DepKind {
    Normal,
    Build,
    Dev,
}

/// Dependencia declarada en un `Cargo.toml`.
#[derive(Clone, Debug)]
pub struct Dep {
    pub name: String,
    pub kind: DepKind,
    pub features: Vec<String>,
    pub default_features: bool,
}

/// Crate del workspace.
#[derive(Clone, Debug, Default)]
pub struct Package {
    pub name: String,
    pub dir: PathBuf,
    pub deps: Vec<Dep>,
    pub features: BTreeMap<String, Vec<String>>,
}

/// Grafo de los crates del workspace.
#[derive(Debug, Default)]
pub struct Graph {
    pub packages: BTreeMap<String, Package>,
}

/// Una regla violada.
#[derive(Debug, PartialEq, Eq)]
pub struct Violation {
    pub rule: String,
    pub message: String,
}

fn matches(pattern: &str, name: &str) -> bool {
    match pattern.strip_suffix('*') {
        Some(prefix) => name.starts_with(prefix),
        None => pattern == name,
    }
}

fn matches_any(patterns: &[String], name: &str) -> bool {
    patterns.iter().any(|p| matches(p, name))
}

impl Graph {
    /// Construye el grafo desde la salida JSON de `cargo metadata --no-deps`.
    pub fn from_metadata(json: &str) -> Result<Self, serde_json::Error> {
        #[derive(Deserialize)]
        struct Meta {
            packages: Vec<MetaPackage>,
        }
        #[derive(Deserialize)]
        struct MetaPackage {
            name: String,
            manifest_path: PathBuf,
            dependencies: Vec<MetaDep>,
            features: BTreeMap<String, Vec<String>>,
        }
        #[derive(Deserialize)]
        struct MetaDep {
            name: String,
            kind: Option<String>,
            features: Vec<String>,
            uses_default_features: bool,
        }

        let meta: Meta = serde_json::from_str(json)?;
        let mut graph = Self::default();
        for p in meta.packages {
            let deps = p
                .dependencies
                .into_iter()
                .map(|d| Dep {
                    name: d.name,
                    kind: match d.kind.as_deref() {
                        Some("dev") => DepKind::Dev,
                        Some("build") => DepKind::Build,
                        _ => DepKind::Normal,
                    },
                    features: d.features,
                    default_features: d.uses_default_features,
                })
                .collect();
            let dir = p
                .manifest_path
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_default();
            graph.packages.insert(
                p.name.clone(),
                Package {
                    name: p.name,
                    dir,
                    deps,
                    features: p.features,
                },
            );
        }
        Ok(graph)
    }

    /// Crates del workspace alcanzables desde `from`, con el camino que lleva a cada uno.
    /// Desde `from` cuentan todas sus dependencias (también las de desarrollo); en los
    /// saltos siguientes, sólo las normales y de build, que son las que se compilan.
    fn reachable(&self, from: &str, transitive: bool) -> BTreeMap<String, Vec<String>> {
        let mut found: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut stack = vec![(from.to_string(), vec![from.to_string()])];
        while let Some((name, path)) = stack.pop() {
            let Some(pkg) = self.packages.get(&name) else {
                continue;
            };
            let first_hop = name == from;
            for dep in &pkg.deps {
                if !first_hop && dep.kind == DepKind::Dev {
                    continue;
                }
                if found.contains_key(&dep.name) || dep.name == from {
                    continue;
                }
                let mut next = path.clone();
                next.push(dep.name.clone());
                found.insert(dep.name.clone(), next.clone());
                if transitive {
                    stack.push((dep.name.clone(), next));
                }
            }
        }
        found
    }

    /// Si `feature` queda activa en `target` por la feature `start` (siguiendo las
    /// features que activa cada una).
    fn feature_enables(&self, target: &str, start: &str, feature: &str) -> bool {
        let Some(pkg) = self.packages.get(target) else {
            return false;
        };
        let mut seen = BTreeSet::new();
        let mut stack = vec![start.to_string()];
        while let Some(f) = stack.pop() {
            if f == feature {
                return true;
            }
            if seen.insert(f.clone())
                && let Some(enables) = pkg.features.get(&f)
            {
                stack.extend(enables.iter().cloned());
            }
        }
        false
    }
}

/// Verifica las reglas de grafo. Las de reloj, con [`check_clock`].
pub fn check_graph(graph: &Graph, rules: &Rules) -> Vec<Violation> {
    let mut out = Vec::new();

    for rule in &rules.forbidden {
        for from in graph.packages.keys().filter(|n| matches_any(&rule.from, n)) {
            for (to, path) in graph.reachable(from, rule.transitive) {
                if matches_any(&rule.to, &to) {
                    out.push(Violation {
                        rule: rule.id.clone(),
                        message: format!("{}: {}", rule.description, path.join(" → ")),
                    });
                }
            }
        }
    }

    for rule in &rules.feature_restricted {
        for pkg in graph.packages.values() {
            if matches_any(&rule.allowed, &pkg.name) {
                continue;
            }
            for dep in pkg
                .deps
                .iter()
                .filter(|d| matches_any(&rule.crates, &d.name))
            {
                let explicit = dep
                    .features
                    .iter()
                    .any(|f| graph.feature_enables(&dep.name, f, &rule.feature));
                let by_default = dep.default_features
                    && graph.feature_enables(&dep.name, "default", &rule.feature);
                if explicit || by_default {
                    let how = if explicit {
                        "explícita"
                    } else {
                        "por `default`"
                    };
                    out.push(Violation {
                        rule: rule.id.clone(),
                        message: format!(
                            "{}: {} → {} con `{}` ({how})",
                            rule.description, pkg.name, dep.name, rule.feature
                        ),
                    });
                }
            }
        }
    }

    out
}

/// Verifica que cada crate de `[clock]` exista y tenga un `clippy.toml` que prohíba
/// todos los métodos de la regla.
pub fn check_clock(graph: &Graph, rule: &ClockRule) -> Vec<Violation> {
    let violation = |message: String| Violation {
        rule: "pilar2-reloj".into(),
        message,
    };
    let mut out = Vec::new();
    for name in &rule.crates {
        let Some(pkg) = graph.packages.get(name) else {
            out.push(violation(format!("{name} no existe en el workspace")));
            continue;
        };
        let path = pkg.dir.join("clippy.toml");
        let listed = match std::fs::read_to_string(&path) {
            Ok(text) => disallowed_methods(&text),
            Err(_) => {
                out.push(violation(format!("{name}: falta {}", path.display())));
                continue;
            }
        };
        for method in &rule.methods {
            if !listed.contains(method) {
                out.push(violation(format!(
                    "{name}: clippy.toml no prohíbe `{method}`"
                )));
            }
        }
    }
    out
}

/// Rutas de `disallowed-methods` de un `clippy.toml` (como texto o como tabla con `path`).
pub fn disallowed_methods(clippy_toml: &str) -> BTreeSet<String> {
    let Ok(value) = clippy_toml.parse::<toml::Table>() else {
        return BTreeSet::new();
    };
    let Some(list) = value.get("disallowed-methods").and_then(|v| v.as_array()) else {
        return BTreeSet::new();
    };
    list.iter()
        .filter_map(|item| match item {
            toml::Value::String(s) => Some(s.clone()),
            toml::Value::Table(t) => t.get("path")?.as_str().map(str::to_string),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules() -> Rules {
        let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/rules.toml"))
            .expect("rules.toml");
        toml::from_str(&text).expect("rules.toml válido")
    }

    fn dep(name: &str) -> Dep {
        Dep {
            name: name.into(),
            kind: DepKind::Normal,
            features: vec![],
            default_features: true,
        }
    }

    fn graph(edges: &[(&str, Vec<Dep>)]) -> Graph {
        let mut g = Graph::default();
        for (name, deps) in edges {
            g.packages.insert(
                (*name).into(),
                Package {
                    name: (*name).into(),
                    deps: deps.clone(),
                    ..Package::default()
                },
            );
        }
        g
    }

    fn rule_ids(g: &Graph) -> Vec<String> {
        check_graph(g, &rules())
            .into_iter()
            .map(|v| v.rule)
            .collect()
    }

    #[test]
    fn rules_file_parses_and_clean_graph_passes() {
        let g = graph(&[
            ("bender-mcp", vec![dep("bender-query")]),
            ("bender-query", vec![dep("bender-types")]),
            ("bender-kernel", vec![dep("bender-types")]),
            ("bender-execution", vec![dep("bender-venue-binance")]),
            ("bender-venue-binance", vec![]),
            ("bender-types", vec![]),
        ]);
        assert_eq!(check_graph(&g, &rules()), vec![]);
    }

    #[test]
    fn mcp_cannot_reach_commands_client_even_transitively() {
        let g = graph(&[
            ("bender-mcp", vec![dep("bender-api")]),
            ("bender-api", vec![dep("bender-commands-client")]),
            ("bender-commands-client", vec![]),
        ]);
        let found = check_graph(&g, &rules());
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].rule, "adr0006-llm-sin-ejecucion");
        assert!(
            found[0]
                .message
                .ends_with("bender-mcp → bender-api → bender-commands-client")
        );
    }

    #[test]
    fn llm_gateway_name_is_covered_too() {
        let g = graph(&[
            ("bender-llm-gateway", vec![dep("bender-execution")]),
            ("bender-execution", vec![]),
        ]);
        assert_eq!(rule_ids(&g), vec!["adr0006-llm-sin-ejecucion"]);
    }

    #[test]
    fn kernel_and_engine_cannot_reach_any_venue_or_execution() {
        let g = graph(&[
            ("bender-kernel", vec![dep("bender-venue-api")]),
            ("bender-engine", vec![dep("bender-store")]),
            ("bender-store", vec![dep("bender-execution")]),
            ("bender-venue-api", vec![]),
            ("bender-execution", vec![]),
        ]);
        let mut ids = rule_ids(&g);
        ids.sort();
        assert_eq!(
            ids,
            vec!["pilar3-analisis-sin-venues", "pilar3-analisis-sin-venues"]
        );
    }

    #[test]
    fn dev_dependencies_of_intermediate_crates_are_not_followed() {
        let mut test_only = dep("bender-execution");
        test_only.kind = DepKind::Dev;
        let g = graph(&[
            ("bender-engine", vec![dep("bender-store")]),
            ("bender-store", vec![test_only.clone()]),
            ("bender-execution", vec![]),
        ]);
        assert_eq!(check_graph(&g, &rules()), vec![]);
        // Pero una dev-dependency directa del crate vigilado sí cuenta.
        let g = graph(&[
            ("bender-engine", vec![test_only]),
            ("bender-execution", vec![]),
        ]);
        assert_eq!(rule_ids(&g), vec!["pilar3-analisis-sin-venues"]);
    }

    #[test]
    fn private_feature_only_from_execution() {
        let mut private = dep("bender-venue-bybit");
        private.features = vec!["private".into()];
        let mut venue = Package {
            name: "bender-venue-bybit".into(),
            ..Package::default()
        };
        venue.features.insert("private".into(), vec![]);

        let mut g = graph(&[
            ("bender-recorder", vec![private.clone()]),
            ("bender-execution", vec![private]),
        ]);
        g.packages.insert(venue.name.clone(), venue.clone());
        let found = check_graph(&g, &rules());
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].rule, "pilar3-trading-privado");
        assert!(
            found[0]
                .message
                .contains("bender-recorder → bender-venue-bybit")
        );

        // También si `private` llega por `default` o por otra feature que la activa.
        venue.features.insert("default".into(), vec!["full".into()]);
        venue.features.insert("full".into(), vec!["private".into()]);
        let mut g = graph(&[("bender-recorder", vec![dep("bender-venue-bybit")])]);
        g.packages.insert(venue.name.clone(), venue);
        let found = check_graph(&g, &rules());
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].message.contains("por `default`"));
    }

    #[test]
    fn clock_rule_requires_clippy_toml_with_every_method() {
        let dir = std::env::temp_dir().join(format!("arch-tests-{}", std::process::id()));
        let ok = dir.join("ok");
        let partial = dir.join("partial");
        let missing = dir.join("missing");
        for d in [&ok, &partial, &missing] {
            std::fs::create_dir_all(d).unwrap();
        }
        let rule = ClockRule {
            crates: vec![
                "ok".into(),
                "partial".into(),
                "missing".into(),
                "nope".into(),
            ],
            methods: vec![
                "std::time::SystemTime::now".into(),
                "chrono::Utc::now".into(),
            ],
        };
        std::fs::write(
            ok.join("clippy.toml"),
            "disallowed-methods = [\"std::time::SystemTime::now\", \
             { path = \"chrono::Utc::now\", allow-invalid = true }]",
        )
        .unwrap();
        std::fs::write(
            partial.join("clippy.toml"),
            "disallowed-methods = [{ path = \"std::time::SystemTime::now\" }]",
        )
        .unwrap();
        let mut g = Graph::default();
        for (name, d) in [("ok", &ok), ("partial", &partial), ("missing", &missing)] {
            g.packages.insert(
                name.into(),
                Package {
                    name: name.into(),
                    dir: d.clone(),
                    ..Package::default()
                },
            );
        }
        let found: Vec<String> = check_clock(&g, &rule)
            .into_iter()
            .map(|v| v.message)
            .collect();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(found.len(), 3, "{found:?}");
        assert_eq!(
            found[0],
            "partial: clippy.toml no prohíbe `chrono::Utc::now`"
        );
        assert!(found[1].starts_with("missing: falta "));
        assert_eq!(found[2], "nope no existe en el workspace");
    }

    #[test]
    fn parses_cargo_metadata() {
        let json = r#"{"packages":[{"name":"a","manifest_path":"/r/crates/a/Cargo.toml",
            "features":{"default":["x"],"x":[]},
            "dependencies":[{"name":"b","kind":"dev","features":["f"],"uses_default_features":false},
                            {"name":"c","kind":null,"features":[],"uses_default_features":true}]}]}"#;
        let g = Graph::from_metadata(json).unwrap();
        let a = &g.packages["a"];
        assert_eq!(a.dir, PathBuf::from("/r/crates/a"));
        assert_eq!(a.deps[0].kind, DepKind::Dev);
        assert!(!a.deps[0].default_features);
        assert_eq!(a.deps[1].kind, DepKind::Normal);
        assert_eq!(a.features["default"], vec!["x"]);
    }
}
