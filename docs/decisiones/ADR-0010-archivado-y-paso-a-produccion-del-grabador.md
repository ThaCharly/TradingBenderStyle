# ADR-0010 — Archivado de datos irreemplazables y paso del grabador a la mini-PC

- Estado: **aceptada**
- Fecha: 2026-10-27
- Amplía: [ADR-0009](ADR-0009-presupuesto-de-disco.md) (la retención de ADR-0009 queda
  aprobada como configuración inicial)

## 1. Archivado comprimido antes de eliminar

### Contexto
ADR-0009 elimina la zona cruda a los 90 días y compacta el L2 a los 30. Parte de esos
datos **no se pueden volver a conseguir**. El usuario pide que exista la opción de
archivarlos comprimidos antes de borrarlos, sin implementar almacenamiento externo desde
el día uno pero dejándolo previsto.

### Qué es irreemplazable
Un segmento es **irreemplazable** si su `provenance = live` y no existe una fuente
histórica pública que lo reproduzca. Clasificación inicial (configurable, versionada):

| Clase | Ejemplos | ¿Irreemplazable? |
|---|---|---|
| `raw.trades.dumpable` | trades de Binance y Bybit (hay dumps públicos) | no |
| `raw.trades.no_dump` | trades de BloFin, BingX, MEXC | **sí** |
| `raw.l2` | deltas de libro de cualquier venue | **sí** |
| `raw.stats` | funding, OI, liquidaciones, mark/index | **sí** (el OI histórico casi nunca es descargable) |
| `raw.clock` | latidos y huecos | **sí** (son pequeños y necesarios para reproducir) |

### Decisión
1. Antes de borrar un segmento de una clase irreemplazable, el proceso de retención lo
   **archiva**: se re-comprime (zstd de nivel alto), se empaqueta por fuente y día con un
   manifiesto de hashes, se **verifica leyéndolo de vuelta** y recién entonces se borra el
   original.
2. **Fallo seguro**: si el archivado falla o el destino no tiene espacio, **no se borra
   nada** y se emite una alerta. Preferimos quedarnos sin disco con alarma a perder datos
   en silencio.
3. **Destinos enchufables** (`ArchiveTarget`):
   - día uno: `LocalDir` (una carpeta; puede ser un disco externo montado);
   - previstos, sin implementar: compatible con S3 / VPS (rclone o similar).
   Cambiar de destino es configuración.
4. **Cuota por clase**: cada clase tiene un tope en el destino. El L2 crudo es lo más
   pesado (estimado: cientos de GB por año sólo para BTC/ETH de Binance), así que **por
   defecto su archivado queda apagado mientras el destino sea el disco local** de 1 TB.
   Se enciende cuando haya un destino externo. Las demás clases irreemplazables, que son
   chicas, se archivan por defecto en local.
5. **Restauración**: una herramienta devuelve segmentos archivados al `bender-store`
   (verificando hashes), así la re-normalización (#38) puede usarlos aunque tengan más de
   90 días.

**El archivado no es un respaldo**: hay una sola copia. Ver
[archivado vs respaldo](../operacion/archivado-vs-respaldo.md).

### Alternativas descartadas
- **Archivar todo**: el L2 crudo llena el disco local en meses.
- **Implementar destino externo ya**: no hace falta hasta que la alarma de disco lo pida.
- **Borrar si el archivado falla**: pérdida silenciosa de datos irreemplazables.

## 2. Paso del grabador a la mini-PC

### Decisión
El grabador corre en la PC de desarrollo hasta pasar un **gate de producción**. Recién ahí
se mueve a la mini-PC. El gate exige:
- 7 días continuos de grabación en la PC con reporte de huecos;
- pruebas de recuperación pasadas: corte de red, cierre forzado del WebSocket por el
  exchange, kill del proceso, reinicio, corte de luz abrupto, disco lleno y salto del
  reloj del sistema. En cada una, el resultado tiene que ser un `Gap` correcto,
  reanudación automática y ningún segmento corrupto;
- retención y archivado funcionando;
- migración de los datos ya grabados con verificación de hashes.

### Consecuencia
Mientras tanto, los apagados de la PC dejan huecos registrados como `Gap`. Se acepta: los
huecos quedan explícitos y la reproducibilidad no se rompe.
