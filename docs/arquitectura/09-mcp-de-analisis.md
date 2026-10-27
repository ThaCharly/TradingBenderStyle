# MCP de análisis (`bender-mcp`)

> El motor, el diario y tu análisis expuestos como un **servidor MCP**. Claude, Codex o el
> LLM local se conectan y pueden ver el mercado en cualquier instante, leer lo que
> escribiste, revisar tus trades y **escribir su propio análisis en una capa separada**.
> Las decisiones siguen siendo tuyas: ningún camino lleva a `execution`.
> Decisión: [ADR-0013](../decisiones/ADR-0013-mcp-de-analisis.md).

---

## 1. Qué es

Un único servidor de herramientas (Model Context Protocol) que sirve a todos los clientes:

| Cliente | Dónde corre | Cómo se conecta |
|---|---|---|
| Claude Code / Claude Desktop | tu PC | stdio (proceso local) o HTTP por Tailscale |
| Codex (CLI) | tu PC | stdio o HTTP por Tailscale |
| LLM local 4B / 9B / 35B (M11) | teléfono / PC | el mismo servidor, mismo catálogo |

Reemplaza al `llm-gateway` como servidor de herramientas: hay **un solo catálogo** para
todos los modelos.

## 2. Catálogo de herramientas (se amplía con cada milestone)

### Lectura

| Grupo | Herramientas | Disponible desde |
|---|---|---|
| Mercado | `list_instruments`, `get_candles(par, tf, desde, hasta)`, `get_coverage`, `get_trades_tape` (acotado) | P0 |
| Tu análisis | `get_drawings`, `get_marked_levels`, `get_user_notes` | P0 |
| Indicadores y perfiles | `get_indicator`, `get_volume_profile(desde, hasta, venues)` | M3 |
| Motor | `get_levels`, `get_events`, `get_level_provenance`, `get_confluences`, `get_channels` | M8 |
| Estadística | `get_estimate(id)`, `list_cases(estimate)`, `get_baselines` | M9 |
| Diario y cuenta | `get_trades`, `get_trade`, `get_stats`, `get_equity_curve`, `get_flags`, `get_rules`, `get_account_state` | M10 (con política de datos, §5) |

### Escritura (sólo en la capa de IA, §4)

| Herramienta | Qué escribe |
|---|---|
| `append_ai_note(sobre: par/trade/período/nivel, texto, etiquetas)` | una nota de análisis |
| `propose_drawing(tipo, puntos, etiqueta, razonamiento)` | un dibujo en la capa de IA (línea, zona, canal, Fibonacci…) |
| `propose_level(precio, tipo, razonamiento)` | un nivel propuesto |
| `propose_hypothesis(pregunta, condicionamiento)` | un borrador para el registro de hipótesis (queda **exploratorio**, M9) |
| `propose_alert(condición, texto)` | una alerta **propuesta**: no se activa hasta que la aceptás |

No existen herramientas que creen, modifiquen o cancelen órdenes, ni que cambien
guardarraíles, configuración o tus propios registros.

## 3. El horizonte temporal: sesiones ancladas

Toda herramienta acepta `as_of`. Además, una conexión puede abrirse **anclada a un
instante T**: todas las herramientas responden como si fuera T, y no hay forma de pedir
datos posteriores. Esto lo garantiza el núcleo temporal (Pilar 2), no una instrucción al
modelo.

Sirve para:
- pedirle a la IA "analizá BTC 4H como si fuera el 10/05/2026 10:00" y comparar después su
  análisis con lo que pasó, sin que haya podido hacer trampa;
- revisar un trade viendo exactamente lo que había cuando entraste.

## 4. La capa de IA

- Todo lo que escribe una IA va a un **espacio propio**: `ai_notes`, `ai_drawings`,
  `ai_levels`, `ai_proposals`. Cada registro guarda qué cliente y qué modelo lo escribió, a
  qué hora, con qué `as_of` y la traza de herramientas que usó.
- Append-only: la IA no edita ni borra, ni siquiera lo suyo; puede **reemplazar** con un
  registro nuevo que referencia al anterior.
- En la gráfica, la capa de IA es **otra capa**, con estilo distinto y apagable. Tus
  dibujos nunca se mezclan con los de la IA.
- **Aceptar** (desde la UI): copia el elemento a tu capa con referencia a su origen. Recién
  ahí cuenta como tuyo. Las alertas propuestas sólo se activan al aceptarlas.
- Todo se versiona como `UserEvent` (con autor `ai:<cliente>`), así la máquina del tiempo
  también muestra lo que la IA había dicho en cada momento.

## 5. Política de datos (salida hacia la nube)

Claude y Codex corren en servidores de Anthropic y OpenAI: **lo que devuelven las
herramientas sale de tu casa**. Por eso cada cliente tiene un **nivel de datos**:

| Nivel | Qué ve |
|---|---|
| `mercado` | velas, cobertura, tus dibujos y notas de análisis, motor, estadística |
| `trades-relativos` | lo anterior + tus trades en **R-múltiplos y %**, sin montos ni saldos |
| `completo` | lo anterior + montos, saldos, capital, PnL en dinero |

- Default para clientes en la nube: `mercado`. Default para el LLM local: `completo`.
- La redacción la hace el servidor, no el modelo.
- Nunca se exponen: claves, ids de cuenta del exchange, direcciones, datos personales.

## 6. Seguridad

Se mantienen las cinco capas de ADR-0006, aplicadas al servidor MCP:
1. **Código**: `bender-mcp` no depende de `bender-commands-client` ni de `execution`
   (test de grafo).
2. **Catálogo cerrado** con snapshot en tests.
3. **Credenciales**: token por cliente con alcances `read:*` y `write:ai_layer`; sin
   `trade:command`; `execution` exige firma de dispositivo con biometría.
4. **Red**: stdio local o HTTP sólo por Tailscale. **No se expone a internet.** Los
   conectores web de claude.ai necesitan una URL pública, así que quedan fuera, salvo una
   ADR nueva.
5. **Presentación**: lo de la capa de IA se ve como propuesta; nunca se convierte en acción
   sin tu aceptación explícita, y aceptar nunca crea órdenes.

Auditoría: cada llamada (cliente, herramienta, argumentos, `as_of`, tamaño de la
respuesta, nivel de datos) queda registrada.
