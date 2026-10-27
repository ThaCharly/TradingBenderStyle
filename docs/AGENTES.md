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

## Definición de terminado (para todo issue)

- [ ] Todos los criterios de aceptación del issue se cumplen y son verificables con un
      comando o un test nombrado en el PR.
- [ ] `make check` pasa.
- [ ] Los tests nuevos corren sin red, salvo los marcados `#[ignore]` de testnet.
- [ ] Ninguna clave, token ni dato personal en el repo.
- [ ] Documentación actualizada si cambió algo especificado.
- [ ] El PR incluye: `Closes #<n>`, la lista de criterios con el test o comando que verifica
      cada uno, y la sección "Huecos de documentación" (vacía si no hubo).
