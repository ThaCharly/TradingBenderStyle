# Aislamiento del LLM

> El LLM tiene **sólo herramientas de consulta** y un único permiso de escritura: agregar
> notas propias al diario. **No existe ningún camino**, directo o indirecto, desde el LLM
> hacia `execution`. Decisión: [ADR-0006](../decisiones/ADR-0006-aislamiento-llm.md).
>
> **Ampliado por [ADR-0013](../decisiones/ADR-0013-mcp-de-analisis.md)**: el servidor de
> herramientas es `bender-mcp` y también lo usan Claude y Codex. La escritura se amplía a la
> capa de IA (notas, dibujos, niveles, propuestas); ver
> [09-mcp-de-analisis.md](09-mcp-de-analisis.md). Todo lo que sigue aplica igual.

Se aplica defensa en profundidad: si falla una capa, las demás siguen cerrando el camino.

---

## Capa 1 — Código: no existe el cliente de ejecución

- El crate `bender-mcp` y el módulo LLM del cliente Android **no dependen** del
  cliente de comandos (`bender-commands-client`). Un test de arquitectura en CI recorre el
  grafo de dependencias (`cargo metadata` y el equivalente en TS) y falla si aparece esa
  arista.
- El registro de herramientas es **una lista cerrada** con snapshot en tests. Agregar una
  herramienta requiere cambiar el snapshot (revisión explícita).

## Capa 2 — Herramientas

El catálogo es el de `bender-mcp`
([09-mcp-de-analisis.md](09-mcp-de-analisis.md) §2): lectura de mercado, análisis del
usuario, motor, estadística y, según el nivel de datos, diario y cuenta.

Escritura, **sólo en la capa de IA** (§4 del doc 09): `append_ai_note`, `propose_drawing`,
`propose_level`, `propose_alert`, `propose_hypothesis`.
- Append-only, atribuida (cliente, modelo, `as_of`, traza de herramientas).
- Límites de tamaño (default 8 KB) y de tasa por cliente.
- No puede editar ni borrar nada, tampoco lo propio.
- No puede modificar tus alertas, dibujos, niveles, notas, guardarraíles ni configuración:
  sólo proponer. Aceptar es una acción tuya.

## Capa 3 — Credenciales

- El token de cada cliente tiene alcances `read:*` (según su nivel de datos) y
  opcionalmente `write:ai_layer`. Nunca `trade:command`.
- `execution` sólo acepta comandos **firmados por una clave de dispositivo**. El LLM no
  tiene ninguna. En el teléfono, la clave está en el Android Keystore y exige
  **biometría por operación**: aunque el LLM corra dentro de la misma app, no puede firmar
  sin tu dedo.
- La pantalla de confirmación de una orden la genera el código de trading a partir del
  comando firmado, nunca a partir de texto del LLM.

## Capa 4 — Red

- `bender-mcp` sólo alcanza `query`, `journal` y la capa de IA (Tailscale ACL + firewall local).
- La base de datos se consulta con un rol de **sólo lectura** sobre vistas específicas; el
  rol de la capa de IA sólo puede hacer `INSERT` en sus tablas (`ai_*`).

## Capa 5 — Presentación

- Lo que escribe el LLM se renderiza como texto. Los enlaces profundos a acciones de
  trading están deshabilitados en contenido generado por el LLM: no hay botones de
  "ejecutar" derivados de su salida.
- En Android, el LLM corre en un **proceso separado** (servicio aislado) que se comunica
  con la app sólo por un *broker* de herramientas que expone la lista cerrada.

## Inyección de instrucciones

Los datos que lee el LLM (notas, textos de exchanges) no son de confianza. Como el LLM no
tiene poder de escritura más allá de sus notas, el daño máximo posible es una nota basura.

## Pruebas obligatorias

1. Grafo de dependencias sin arista LLM → comandos.
2. Snapshot del registro de herramientas.
3. Con el token del LLM, cualquier endpoint de comandos responde 403.
4. Desde el contenedor/proceso del LLM, la conexión a `execution` es rechazada.
5. Con el rol de notas, `UPDATE`/`DELETE` y escrituras en otras tablas fallan.
6. Un comando firmado con una clave que no es de dispositivo emparejado es rechazado.
