# Archivado no es respaldo

Son dos cosas distintas, que protegen contra riesgos distintos. Hoy existe el diseño del
**archivado** ([ADR-0010](../decisiones/ADR-0010-archivado-y-paso-a-produccion-del-grabador.md)).
Un sistema de **respaldo** todavía **no** existe; esta página documenta la diferencia y
qué queda descubierto.

## Diferencia

| | Archivado | Respaldo |
|---|---|---|
| Para qué | liberar espacio caliente conservando datos que se van a borrar | sobrevivir a la pérdida del original |
| Cuántas copias | **una**: el dato se mueve, el original se borra | **al menos dos**: el original sigue existiendo |
| Dónde | puede estar en el mismo disco o la misma máquina | **otro dispositivo**, idealmente otro lugar |
| Qué cubre | sólo datos viejos de ciertas clases | todo lo que no se puede reconstruir |
| Protege contra | quedarse sin disco | falla del disco, borrado accidental, corrupción, robo, ransomware, incendio |
| Prueba | leer de vuelta y verificar hashes al archivar | **restaurar periódicamente** en otra máquina |

Regla práctica: **si un dato vive en un solo dispositivo, no está respaldado**, aunque
esté archivado y comprimido.

## Qué queda descubierto hoy (riesgo aceptado temporalmente)

Si muere el disco de la máquina que graba, se pierde:
- toda la historia grabada en vivo (lo irreemplazable: L2, BloFin/BingX/MEXC, OI, latidos);
- el archivo, si está en el mismo disco;
- dibujos, configuración y, más adelante, diario, ledger y claves cifradas.

Lo que **sí** se puede reconstruir sin respaldo: el código (GitHub), los dumps históricos
públicos y todo lo derivado (velas, rollups, features, modelos), que es re-calculable.

## Qué tendrá que respaldarse, por prioridad (cuando se implemente)

1. **Keystore cifrado y configuración de `execution`** (cuando exista M6).
2. **Postgres**: ledger, órdenes, diario, dibujos, anotaciones, registro de hipótesis.
3. **Datos crudos y normalizados `live`/`recovered`** (lo irreemplazable).
4. El archivo.

Esquema de referencia: 3-2-1 (tres copias, dos medios, una fuera de casa), con copias
cifradas del lado del cliente y verificación de restauración.

## Disparador para implementarlo

Antes de que el grabador pase a la mini-PC como producción (gate #140) se registra la
decisión de respaldo (aunque sea "todavía no", con la justificación), y **antes de operar
con dinero real** (M6/M7) el respaldo de Postgres y del keystore es obligatorio.
