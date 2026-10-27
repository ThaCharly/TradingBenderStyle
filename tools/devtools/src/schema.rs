//! Chequeos del esquema Protobuf (`schema/`) que `buf lint` no cubre:
//!
//! - Pilar 1: ningún `float`/`double` (ni sus wrappers) en el esquema.
//! - Lo desconocido es explícito: todo campo escalar de un mensaje tiene presencia
//!   (`optional`, `repeated` o dentro de un `oneof`) o está marcado `[requerido]` en su
//!   comentario. Los mensajes ya tienen presencia; los enums usan `*_UNSPECIFIED` = 0
//!   como "desconocido", lo que exige `buf lint`.

use std::path::{Path, PathBuf};

/// Marca que declara obligatorio un escalar sin presencia.
pub const REQUIRED_MARK: &str = "[requerido]";

const SCALARS: &[&str] = &[
    "int32", "int64", "uint32", "uint64", "sint32", "sint64", "fixed32", "fixed64", "sfixed32",
    "sfixed64", "bool", "string", "bytes",
];

/// Un problema encontrado en un `.proto`.
#[derive(Debug, PartialEq, Eq)]
pub struct SchemaIssue {
    pub file: PathBuf,
    pub line: usize,
    pub message: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Block {
    Message,
    Enum,
    Oneof,
    Other,
}

/// Revisa el texto de un `.proto`. `file` sólo se usa para reportar.
pub fn check_proto(file: &Path, text: &str) -> Vec<SchemaIssue> {
    let mut issues = Vec::new();
    let mut stack: Vec<Block> = Vec::new();
    let mut leading_comment = String::new();

    for (index, raw) in text.lines().enumerate() {
        let line_no = index + 1;
        let (code, comment) = match raw.find("//") {
            Some(pos) => (&raw[..pos], &raw[pos + 2..]),
            None => (raw, ""),
        };
        let code = code.trim();
        let issue = |message: String| SchemaIssue {
            file: file.to_path_buf(),
            line: line_no,
            message,
        };

        for token in code.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '.')) {
            let lower = token.to_ascii_lowercase();
            if ["double", "float", "doublevalue", "floatvalue"]
                .iter()
                .any(|f| lower == *f || lower.ends_with(&format!(".{f}")))
            {
                issues.push(issue(format!(
                    "`{token}` prohibido: usar bender.v1.Decimal (Pilar 1)"
                )));
            }
        }

        if code.is_empty() {
            if comment.is_empty() && raw.trim().is_empty() {
                leading_comment.clear();
            } else {
                leading_comment.push_str(comment);
                leading_comment.push('\n');
            }
            continue;
        }

        let first = code.split_whitespace().next().unwrap_or_default();
        if code.contains('{') {
            let block = match first {
                "message" => Block::Message,
                "enum" => Block::Enum,
                "oneof" => Block::Oneof,
                _ => Block::Other,
            };
            if !code.contains('}') {
                stack.push(block);
            }
        } else if code.starts_with('}') {
            stack.pop();
        } else if stack.last() == Some(&Block::Message)
            && !matches!(first, "reserved" | "option" | "extensions")
        {
            let mut words = code.split_whitespace();
            let label = words.next().unwrap_or_default();
            let ty = (!matches!(label, "optional" | "repeated") && !label.starts_with("map<"))
                .then_some(label);
            if let Some(ty) = ty
                && SCALARS.contains(&ty)
                && !leading_comment.contains(REQUIRED_MARK)
                && !comment.contains(REQUIRED_MARK)
            {
                let name = words.next().unwrap_or_default();
                issues.push(issue(format!(
                    "escalar `{ty} {name}` sin presencia: usar `optional` o marcarlo {REQUIRED_MARK}"
                )));
            }
        }
        leading_comment.clear();
    }
    issues
}

/// Revisa todos los `.proto` bajo `dir`.
pub fn check_schema_dir(dir: &Path) -> std::io::Result<Vec<SchemaIssue>> {
    let mut files = Vec::new();
    collect_protos(dir, &mut files)?;
    files.sort();
    let mut issues = Vec::new();
    for file in files {
        let text = std::fs::read_to_string(&file)?;
        let shown = file.strip_prefix(dir).unwrap_or(&file);
        issues.extend(check_proto(shown, &text));
    }
    Ok(issues)
}

fn collect_protos(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_protos(&path, out)?;
        } else if path.extension().is_some_and(|ext| ext == "proto") {
            out.push(path);
        }
    }
    Ok(())
}

/// Valores de un enum del esquema sin el prefijo del tipo y sin `*_UNSPECIFIED`.
/// `ORDER_STATUS_NEW` en `enum OrderStatus` → `NEW`.
pub fn enum_values(proto: &str, enum_name: &str) -> Vec<String> {
    let header = format!("enum {enum_name} {{");
    let prefix: String = enum_name
        .chars()
        .enumerate()
        .flat_map(|(i, c)| {
            let sep = (i > 0 && c.is_ascii_uppercase()).then_some('_');
            sep.into_iter()
                .chain(std::iter::once(c.to_ascii_uppercase()))
        })
        .chain(std::iter::once('_'))
        .collect();
    proto
        .lines()
        .skip_while(|l| l.trim() != header)
        .skip(1)
        .take_while(|l| l.trim() != "}")
        .filter_map(|l| l.split("//").next()?.trim().split_once('='))
        .filter_map(|(name, _)| name.trim().strip_prefix(&prefix).map(str::to_string))
        .filter(|v| v != "UNSPECIFIED")
        .collect()
}

/// Nombres entre comillas invertidas de la línea del Markdown que empieza con `marker`.
pub fn doc_list(markdown: &str, marker: &str) -> Vec<String> {
    markdown
        .lines()
        .find(|l| l.starts_with(marker))
        .map(|l| {
            l.split('`')
                .skip(1)
                .step_by(2)
                .skip(1) // el nombre del tipo, p. ej. `OrderStatus`
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn messages(text: &str) -> Vec<String> {
        check_proto(Path::new("x.proto"), text)
            .into_iter()
            .map(|i| format!("{}: {}", i.line, i.message))
            .collect()
    }

    #[test]
    fn rejects_floats_and_wrappers_outside_comments() {
        let text = "message A {\n  double a = 1;\n  optional float b = 2;\n  \
                    google.protobuf.DoubleValue c = 3;\n  // un double en un comentario\n}\n";
        let found = messages(text);
        assert_eq!(found.len(), 3, "{found:?}");
        assert!(found[0].starts_with("2: `double`"));
        assert!(found[1].starts_with("3: `float`"));
        assert!(found[2].starts_with("4: `google.protobuf.DoubleValue`"));
    }

    #[test]
    fn scalar_needs_presence_or_required_mark() {
        let text = "message A {\n  int64 sin_marca = 1;\n  // Algo. [requerido]\n  \
                    int64 marcado_arriba = 2;\n  int64 marcado_al_lado = 3; // [requerido]\n  \
                    optional int64 opcional = 4;\n  repeated string lista = 5;\n  \
                    Decimal mensaje = 6;\n  oneof o {\n    string en_oneof = 7;\n  }\n}\n";
        assert_eq!(
            messages(text),
            vec![
                "2: escalar `int64 sin_marca` sin presencia: usar `optional` o marcarlo [requerido]"
            ]
        );
    }

    #[test]
    fn mark_does_not_leak_across_fields_or_into_enums() {
        let text = "message A {\n  // [requerido]\n  string a = 1;\n  string b = 2;\n}\n\
                    enum E {\n  E_UNSPECIFIED = 0;\n}\nmessage Vacio {}\n";
        let found = messages(text);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].starts_with("4: escalar `string b`"));
    }

    #[test]
    fn extracts_enum_values_and_doc_lists() {
        let proto = "enum OrderStatus {\n  ORDER_STATUS_UNSPECIFIED = 0;\n  // x\n  \
                     ORDER_STATUS_NEW = 1;\n  ORDER_STATUS_UNKNOWN = 2; // y\n}\n";
        assert_eq!(enum_values(proto, "OrderStatus"), vec!["NEW", "UNKNOWN"]);
        let md = "texto\nEstados de `OrderStatus`: `NEW`, `UNKNOWN`.\n";
        assert_eq!(doc_list(md, "Estados de"), vec!["NEW", "UNKNOWN"]);
    }

    /// Criterio de #18: los estados coinciden 1:1 con doc 06 §3–4.
    #[test]
    fn trading_states_match_doc06() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let proto = std::fs::read_to_string(root.join("schema/bender/v1/trading.proto")).unwrap();
        let doc = std::fs::read_to_string(root.join("docs/arquitectura/06-ejecucion-futuros.md"))
            .unwrap();
        for name in ["OrderStatus", "SagaState"] {
            let in_doc = doc_list(&doc, &format!("Estados normativos de `{name}`"));
            assert!(!in_doc.is_empty(), "falta la lista de {name} en doc 06");
            assert_eq!(enum_values(&proto, name), in_doc, "{name}");
        }
    }

    #[test]
    fn repo_schema_passes() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schema");
        assert_eq!(check_schema_dir(&dir).unwrap(), vec![]);
    }
}
