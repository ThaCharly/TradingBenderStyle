# Guía para agentes de programación

Cada issue está pensado para que un agente lo implemente **solo**, sin contexto de
conversaciones previas. Antes de empezar:

1. Leé el issue completo y **todos** los documentos que enlaza en "Referencias".
2. Verificá que los issues de "Bloqueado por" estén cerrados. Si no lo están, no empieces.
3. Trabajá en una rama `issue-<número>-<slug>` creada desde `main` y abrí un PR hacia `main`
   que diga `Closes #<número>`. Detalle en [RAMAS.md](RAMAS.md).

## Entorno

```sh
make setup   # una vez
make check   # antes de cada PR: lo mismo que la CI
```

Requisitos y detalles en el [README](../README.md) y en
[00-estructura-del-repo.md](arquitectura/00-estructura-del-repo.md).

## Alcance

- Implementá **sólo** lo que pide el issue. No adelantes funcionalidades de otros issues
  aunque parezcan obvias; si algo falta para cumplir un criterio, comentalo.
- Si la documentación no alcanza para decidir algo, **no inventes**: anotá la pregunta en el
  PR (sección "Huecos de documentación") y elegí la opción más conservadora, o pará.

## Reglas que no se negocian

- **Pilar 2**: ningún crate de `bender-kernel`, `bender-engine`, `bender-runner` ni
  `bender-venue-sim` lee el reloj del sistema. El lint de CI lo bloquea.
- **Pilar 1**: nada de `f32`/`f64` para precios, cantidades, comisiones ni saldos. Usá
  `bender_types::Decimal`.
- **Pilar 3**: sólo `bender-execution` puede depender de los crates de venues con
  capacidades de trading privado o leer claves. El test de arquitectura lo verifica.
- **ADR-0006 y ADR-0013**: `bender-mcp` nunca depende de `bender-commands-client` ni de `bender-execution`.
- Si el issue contradice un documento de `docs/arquitectura`, **no improvises**: comentá
  en el issue y pará.
- Si cambiás un comportamiento especificado, actualizá el documento en el mismo PR.

## Tests de arquitectura

Las reglas anteriores las verifica la CI desde `make rust` y `make ts`. Se declaran en
[`tools/arch-tests/rules.toml`](../tools/arch-tests/rules.toml) (Rust) y
[`tools/arch-tests/eslint-rules.js`](../tools/arch-tests/eslint-rules.js) (TypeScript).

| Regla | Qué prohíbe | Cómo se verifica |
|---|---|---|
| `pilar2-reloj` | Leer el reloj del sistema (`SystemTime::now`/`elapsed`, `Instant::now`/`elapsed`, `chrono::{Utc,Local}::now`, `ulid::Ulid::new`) en `bender-kernel`, `bender-engine`, `bender-runner` y `bender-venue-sim` | Cada uno tiene un `clippy.toml` con `disallowed-methods`. El lint es `forbid` en el workspace, así que `#[allow]` no lo apaga. `arch-tests` comprueba que los `clippy.toml` existan y estén completos. |
| `pilar3-trading-privado` | Depender de `bender-venue-{binance,blofin,bingx,bybit,mexc}` con la feature `private` desde cualquier crate que no sea `bender-execution`. Cuenta tanto si se pide explícitamente como si llega por `default` | `cargo run -p arch-tests` sobre `cargo metadata` |
| `adr0006-llm-sin-ejecucion` | Que `bender-mcp` (o el antiguo `bender-llm-gateway`) dependa, directa o transitivamente, de `bender-commands-client` o `bender-execution` | `cargo run -p arch-tests` |
| `pilar3-analisis-sin-venues` | Que `bender-kernel` o `bender-engine` dependan, directa o transitivamente, de un `bender-venue-*` o de `bender-execution` | `cargo run -p arch-tests` |
| ADR-0006 en TS | Que el módulo LLM del cliente (`apps/client/packages/llm*`, `apps/shell/**/llm/`) importe `@bender/commands-client` | ESLint `no-restricted-imports` (`pnpm -r lint`) |

En las reglas de grafo, del crate vigilado cuentan todas sus dependencias (también las de
desarrollo). En los saltos siguientes cuentan sólo las normales y de build. Para agregar un
crate a la regla de reloj, sumalo a `[clock]` en `rules.toml` y copiale el `clippy.toml`
de `crates/bender-kernel`. Si un crate nuevo cae bajo una regla y la necesita romper, eso
es un cambio de arquitectura: se registra como ADR, no se edita la regla.

## Definición de terminado (para todo issue)

- [ ] Todos los criterios de aceptación del issue se cumplen y son verificables con un
      comando o un test nombrado en el PR.
- [ ] `make check` pasa.
- [ ] Los tests nuevos corren sin red, salvo los marcados `#[ignore]` de testnet.
- [ ] Ninguna clave, token ni dato personal en el repo.
- [ ] Documentación actualizada si cambió algo especificado.
- [ ] El PR incluye: `Closes #<n>`, la lista de criterios con el test o comando que verifica
      cada uno, y la sección "Huecos de documentación" (vacía si no hubo).
