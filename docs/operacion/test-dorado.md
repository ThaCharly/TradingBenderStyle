# Test dorado: vivo = replay

Es la tercera prueba normativa del núcleo temporal
([doc 05 §11](../arquitectura/05-nucleo-temporal.md#11-pruebas-normativas)). Verifica que lo
que el sistema emitió en vivo un día se reproduce idéntico al replayar la grabación de ese
día.

## Cómo funciona

1. La corrida en vivo toma, al inicio de cada día UTC, el checkpoint `kernel` del núcleo
   y lo guarda en el store (`checkpoints/kernel/AAAA-MM-DD.bin`). Durante el día anota el
   hash de las salidas que va emitiendo (`DayHash`, una cadena BLAKE3 que empieza en cero a
   las 00:00 UTC). Al cerrar el día escribe una línea `AAAA-MM-DD <hash> <eventos> <salidas>`
   (`golden::day_line`).
2. `bender-golden` abre el store, carga el checkpoint del inicio del día (si no hay,
   replaya desde el principio de lo grabado hasta ese día), replaya los eventos del día
   desde la zona normalizada con el consumidor de velas y compara el hash con el anotado.

## Cómo correrlo (en el servidor, cada noche)

```bash
cargo run --release -p bender-runner --bin bender-golden -- \
  --store /ruta/al/store --day 2026-10-27 --expected <hash anotado en vivo>
```

- Sale con `0` e imprime `OK: vivo = replay` si coinciden.
- Sale con `1` e imprime `DIFERENCIA` si no coinciden. Hay que investigarlo: se rompió el
  Pilar 2.
- Sale con `2` ante un error de uso o de lectura.

Para correrlo todas las noches, la línea del día la escribe el Runner en vivo (#155). El
cron del servidor se configura en el gate de producción (#140).

## Las otras cuatro pruebas

Corren en la CI con `cargo test` (`crates/bender-runner/tests/normative.rs`). Cualquier
consumidor nuevo (motor, alertas) se registra con una línea:

```rust
register_consumer_for_normative_tests!(mi_consumidor, || MiConsumidor::default());
```

El consumidor tiene que implementar `Consumer + Checkpointable`. La doble corrida entre
máquinas se verifica con el hash fijado en `double_run_hash_is_pinned`, que la CI corre en
otra máquina. Entre el servidor y la PC, alcanza con correr el mismo test en las dos.
