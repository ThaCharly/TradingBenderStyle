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

Sólo lectura:
`get_trades`, `get_trade`, `get_stats`, `get_equity_curve`, `get_account_state`,
`get_engine_snapshot`, `get_events`, `get_levels`, `get_estimate`, `search_journal`,
`get_flags`, `get_rules`.

Una sola escritura: `append_assistant_note(trade_id? | period?, text, tags[])`.
- Append-only, en el espacio `assistant_notes`, con autor `llm`, versión del modelo y
  la traza de herramientas que usó.
- Límite de tamaño (default 8 KB por nota) y de tasa (default 30 notas por hora).
- No puede editar ni borrar nada, tampoco sus propias notas.
- No puede crear ni modificar alertas, dibujos, niveles, guardarraíles ni configuración.

## Capa 3 — Credenciales

- El token del LLM tiene alcances `read:market`, `read:engine`, `read:account`,
  `read:journal` y `write:assistant_notes`. Nada más.
- `execution` sólo acepta comandos **firmados por una clave de dispositivo**. El LLM no
  tiene ninguna. En el teléfono, la clave está en el Android Keystore y exige
  **biometría por operación**: aunque el LLM corra dentro de la misma app, no puede firmar
  sin tu dedo.
- La pantalla de confirmación de una orden la genera el código de trading a partir del
  comando firmado, nunca a partir de texto del LLM.

## Capa 4 — Red

- `bender-mcp` sólo alcanza `query`, `journal` y la capa de IA (Tailscale ACL + firewall local).
- La base de datos se consulta con un rol de **sólo lectura** sobre vistas específicas; el
  rol de notas sólo puede hacer `INSERT` en `assistant_notes`.

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
