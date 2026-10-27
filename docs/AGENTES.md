# Guía para agentes de programación

Cada issue está pensado para que un agente lo implemente **solo**, sin contexto de
conversaciones previas. Antes de empezar:

1. Leé el issue completo y **todos** los documentos que enlaza en "Referencias".
2. Verificá que los issues de "Bloqueado por" estén cerrados. Si no lo están, no empieces.
3. Trabajá en una rama `issue-<número>-<slug>` y abrí un PR que diga `Closes #<número>`.

## Reglas que no se negocian

- **Pilar 2**: ningún crate de `bender-kernel`, `bender-engine`, `bender-runner` ni
  `bender-venue-sim` lee el reloj del sistema. El lint de CI lo bloquea.
- **Pilar 1**: nada de `f32`/`f64` para precios, cantidades, comisiones ni saldos. Usá
  `bender_types::Decimal`.
- **Pilar 3**: sólo `bender-execution` puede depender de los crates de venues con
  capacidades de trading privado o leer claves. El test de arquitectura lo verifica.
- **ADR-0006**: `bender-llm-gateway` nunca depende de `bender-commands-client`.
- Si el issue contradice un documento de `docs/arquitectura`, **no improvises**: comentá
  en el issue y pará.
- Si cambiás un comportamiento especificado, actualizá el documento en el mismo PR.

## Definición de terminado (para todo issue)

- [ ] Todos los criterios de aceptación del issue se cumplen y son verificables con un
      comando o un test nombrado en el PR.
- [ ] `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test --workspace` (y los
      equivalentes de TS/Python si aplica) pasan.
- [ ] Los tests nuevos corren sin red, salvo los marcados `#[ignore]` de testnet.
- [ ] Ninguna clave, token ni dato personal en el repo.
- [ ] Documentación actualizada si cambió algo especificado.
