//! Tipos base del dominio. El contenido real llega con el issue #19 (M0-04).

/// Versión del esquema de datos que implementa este crate.
pub const SCHEMA_PACKAGE   :   &str="bender.v1";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_package_is_v1() {
        assert_eq!(SCHEMA_PACKAGE, "bender.v1");
    }
}
