# Pilar 1 — Modelo de datos universal

> Una orden, una posición, una ejecución y un evento de mercado tienen **una sola
> representación interna**, igual para Binance, BloFin, BingX, MEXC, Bybit y Paper.
> Las rarezas de cada exchange mueren en su adaptador y nunca llegan al resto del sistema.

---

## 1. Reglas generales

1. **Esquema definido una sola vez.** Un IDL (Protobuf o similar) del que se generan los
   tipos para Rust (daemon, motor, research) y TypeScript (UI). Nadie escribe
   tipos a mano en dos lenguajes.
2. **Nada de floats para plata.** Precios, cantidades, comisiones, saldos y PnL son
   decimales de punto fijo. Los floats sólo existen dentro de features analíticas.
3. **Lo desconocido es explícito.** Si un exchange no informa un campo, el valor es
   `Unknown`, nunca `0` ni `""`. Un cero inventado es un bug silencioso.
4. **Todo cambio de estado es un evento.** Órdenes, posiciones y saldos no se "pisan": se
   derivan de un log de eventos inmutable (event sourcing). El estado actual es una
   proyección del log. Esto es lo que hace posible el Pilar 2.
5. **Lo crudo se conserva.** Cada mensaje del exchange se guarda tal cual llegó, además
   de su versión normalizada. Si el normalizador tenía un bug, se re-normaliza.
6. **El esquema se versiona.** Cambios incompatibles = versión nueva + migración; el
   histórico viejo sigue siendo legible.

---

## 2. Identidad

| Concepto | Ejemplo | Notas |
|---|---|---|
| `Asset` | `BTC`, `ETH`, `USDT`, `USDC`, `USD` | Monedas/activos. |
| `QuoteFamily` | `USD-like` = {USD, USDT, USDC, FDUSD…} | Para el análisis canónico. La moneda real de cotización se conserva siempre. |
| `CanonicalPair` | `BTC/USD` | Lo que mira el motor y donde viven los dibujos. |
| `Venue` | `BINANCE`, `BLOFIN`, `BINGX`, `MEXC`, `BYBIT`, `PAPER` | |
| `MarketType` | `SPOT`, `PERP_LINEAR`, `PERP_INVERSE`, `FUTURE_DATED` | |
| `InstrumentId` | `BINANCE:PERP_LINEAR:BTCUSDT` | Instrumento concreto en el que se opera. |
| `AccountId` | `BINANCE:principal:futures` | Cuenta/subcuenta y billetera. |

IDs internos: ULID (ordenables por tiempo). Los IDs del exchange se guardan aparte,
nunca se usan como clave primaria.

---

## 3. Especificación de instrumento (versionada en el tiempo)

Los exchanges cambian el tick size, el tamaño de contrato y los límites. Por eso la
especificación tiene vigencia: `valid_from` / `valid_to`.

- `tick_size`, `lot_step`, `min_qty`, `min_notional`
- `contract_size` y unidad (muchos perps cuentan en "contratos" de 0.001 BTC, otros en
  BTC, otros en USD). **Internamente toda cantidad es en unidades del activo base.** La
  conversión a contratos vive sólo en el adaptador.
- lineal/inverso, moneda de liquidación (`settle_asset`)
- escalones de apalancamiento y margen de mantenimiento
- fuente de precio para triggers disponible (last / mark / index)
- estado: operando, pausado, deslistado (los deslistados **no se borran**: evita sesgo de
  supervivencia)

Consultar un instrumento siempre es `spec(instrument, as_of = T)`.

---

## 4. Tiempo

Todo en UTC, nanosegundos. Cada registro lleva **tres marcas**:

| Campo | Qué es |
|---|---|
| `ts_event` | cuándo pasó según el exchange |
| `ts_received` | cuándo llegó a nuestro servidor (reloj sincronizado por NTP/chrony) |
| `ts_available` | cuándo el sistema pudo usarlo. En vivo = `ts_received`. En histórico reconstruido = estimado (ver Pilar 2) |

Más `venue_seq` (número de secuencia del exchange, si existe) e `ingest_seq` (nuestro,
monótono). Con eso hay un **orden total** de eventos, reproducible.

---

## 5. Eventos de mercado

Una unión cerrada (`MarketEvent`):

- `Trade { price, qty_base, aggressor_side, venue_trade_id }`
- `BookSnapshot { bids, asks, venue_seq }` / `BookDelta { changes, venue_seq }`
- `TopOfBook { bid, bid_qty, ask, ask_qty }`
- `MarkPrice`, `IndexPrice`
- `Funding { rate, interval, next_funding_time }`
- `OpenInterest { qty_base }`
- `Liquidation { side, price, qty_base }` (cuando el exchange lo publica)
- `InstrumentStatus { status }` (pausa, deslistado, cambio de spec)
- **`Gap { gap_id, from, to, reason }`** — "se cortó la conexión, no sé qué pasó en este
  intervalo". Es un evento de primera clase: el motor tiene que comportarse igual en vivo
  y en replay frente a un hueco.
- **`GapRecovery { gap_id, recovered_from, recovered_to, method, result }`** — resultado del
  intento de recuperar el hueco desde el proveedor (`full` | `partial` | `none`). No borra
  el `Gap`. Ver [ADR-0011](../decisiones/ADR-0011-completitud-de-datos-y-recuperacion-de-huecos.md).

Las **velas no son eventos de mercado**: son derivadas de los trades. Las velas que
publica el exchange se guardan sólo para validar nuestra agregación.

---

## 6. Trading

### 6.1 Intención → órdenes

- **`OrderIntent`**: lo que vos pediste desde la UI. Ej.: "long BTC, riesgo 1%, entrada
  en escalera 3 órdenes entre 88k y 87k, SL 86k, TP1 2R 50%, TP2 3R resto". Lleva el
  dispositivo que lo firmó (Pilar 3), una clave de idempotencia y la versión del ticket.
- Un intent genera **varias `Order`** agrupadas (`OrderGroup`: bracket, OCO, ladder).

### 6.2 Orden

```
Order
  id (ULID), client_order_id, venue_order_id?
  intent_id, group_id?, role (entry | stop_loss | take_profit | ...)
  instrument, account
  side (buy | sell), position_side (net | long | short)
  type (limit | market | stop_market | stop_limit | take_profit_market | trailing ...)
  time_in_force (GTC | IOC | FOK | post_only)
  price?, trigger_price?, trigger_source (last | mark | index)?
  qty_base, reduce_only
  residence (venue_native | server_trigger)
  status  ← proyección de OrderEvent[]
```

Máquina de estados única para todos los exchanges:

```
created → submitted → accepted → partially_filled → filled
                   ↘ rejected    ↘ canceled / expired
          (server_trigger: created → armed → triggered → submitted …)
```

Los nombres canónicos de los estados (`NEW`, `PENDING_SUBMIT`, …, `UNKNOWN`) son los de
[doc 06 §3](06-ejecucion-futuros.md#3-máquina-de-estados-de-una-orden).

Cada transición es un `OrderEvent` con el mensaje crudo del exchange adjunto. Si un
exchange no informa un estado intermedio, se infiere y se marca como inferido.

### 6.3 Ejecución (fill)

```
Fill
  id, order_id, venue_trade_id
  price, qty_base
  fee { amount, asset }, liquidity (maker | taker)
  ts_event, ts_received
```

### 6.4 Libro contable (ledger)

Todo movimiento de plata es un asiento en un **ledger por cuenta**: fill, comisión,
funding, liquidación, ADL, transferencia, depósito, retiro, rebate. Los saldos y el PnL
son la suma del ledger. Por eso el PnL es auditable centavo por centavo.

### 6.5 Posición

Se **calcula nosotros** a partir de fills + ledger (cantidad, precio promedio, PnL
realizado/no realizado contra mark). Lo que reporta el exchange se usa para
**reconciliar**: si difieren, alerta y se registra la discrepancia. Precio de
liquidación: el reportado por el exchange y el estimado propio, ambos visibles.

---

## 7. Adaptadores

Cada exchange = un adaptador con dos caras:

- **Normalizador**: crudo → modelo canónico. Puro y determinista (mismo input, mismo
  output), versionado.
- **Traductor de comandos**: `Order` canónica → request del exchange, respetando la
  matriz de capacidades.

**Suite de conformidad común**: un set de escenarios grabados (fill parcial y cancelación,
rechazo por reduce-only, cambio de tick size, reconexión con huecos, liquidación,
funding) que **todos** los adaptadores tienen que pasar produciendo exactamente el mismo
resultado canónico. Un adaptador nuevo no entra si no pasa la suite.

Prioridad de adaptadores: **Binance** (spot + perp) → **BloFin** → **BingX** →
**MEXC / Bybit**. Paper existe desde el día uno y también pasa la suite.
