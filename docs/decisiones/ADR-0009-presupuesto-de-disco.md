# ADR-0009 — Presupuesto de disco y retención (~1 TB en la mini-PC)

- Estado: **aceptada como configuración inicial**, se revisa con las mediciones de #32
- Ampliada por: [ADR-0010](ADR-0010-archivado-y-paso-a-produccion-del-grabador.md) (archivado antes de borrar; gate de la mini-PC)
- Fecha: 2026-10-27
- Número: la ADR-0008 queda reservada para el spike #45 (shell móvil y push)

## Contexto

La mini-PC tiene ~1 TB. El grabador corre desde el día uno, y lo que graba (y por cuánto
tiempo) se decide **ahora**, porque condiciona el formato de #32 y la operación de #43.

Estimaciones de **orden de magnitud**, a confirmar con la corrida de 24 h de #32:

| Dato | Volumen aproximado |
|---|---|
| Trades de BTC y ETH, spot + perps, 5 venues, normalizados | ~0,2–0,4 GB/día → ~70–150 GB/año |
| Zona cruda de esos trades (JSON comprimido) | ~2× la normalizada |
| Libro L2 top-50 con deltas, **por instrumento** activo | ~0,2–0,5 GB/día → **el problema**: con 20+ instrumentos, 1–2 TB/año |
| Backfill (Binance BTC/ETH desde 2017/2019, Bitstamp, Coinbase, Bybit) | ~100–200 GB una vez |
| Sistema, Postgres, checkpoints, rollups | ~50–100 GB |

## Decisión (defaults configurables)

1. **Trades: para siempre**, en la zona normalizada.
2. **Zona cruda: 90 días** para todo. Pasado ese plazo, la zona normalizada es la verdad
   de largo plazo. Para que un bug de normalizador se detecte dentro de esa ventana, el
   control de calidad (#42) corre a diario.
3. **Libro L2**:
   - deltas top-20 completos **sólo** para BTC y ETH de Binance (spot + perps), durante
     30 días;
   - después, snapshots top-20 cada 1 s, para siempre;
   - resto de los venues: sólo top-of-book.
4. **Alarma de disco** (#43) cuando la proyección baje de 90 días de espacio. Ahí se
   decide la ampliación.

Con esto, 1 TB alcanza para varios años de BTC/ETH (estimado; se recalcula con datos
reales).

## Alternativas descartadas o diferidas

- **Guardar todo para siempre**: llena 1 TB en meses por el L2.
- **No grabar L2**: se pierde para siempre el heatmap y el estudio de liquidez; el L2
  histórico no se puede descargar después.
- **Zona cruda para siempre**: duplica el uso; el beneficio después de 90 días es bajo.
- **Ampliación** (diferida, disparador en §4): un **disco externo/USB** como almacenamiento
  frío es la opción más barata por TB. Un **VPS** conviene si además se quiere el grabador
  fuera de casa (mejor uptime), pero el almacenamiento en VPS es caro por TB y los datos
  igual tienen que bajar a casa para entrenar.

## Recomendación de despliegue (no bloqueante)

El desarrollo se hace en la PC local (`dev-all`). Pero **apenas #32 funcione, conviene
correr sólo el grabador en la mini-PC**, aunque todo lo demás siga en la PC: la PC se
apaga, y cada hora apagada es un hueco de historia que no se recupera para los venues sin
dumps públicos (BloFin, BingX, MEXC) ni para el L2 de ninguno.

## Consecuencias

- La re-normalización desde crudo (#38) sólo cubre los últimos 90 días.
- #32 debe reportar los tamaños reales por fuente; si difieren mucho de esta tabla, esta
  ADR se actualiza con una nueva.
