# Topología distribuida: servidor doméstico, PC y Android

> Cada componente puede correr en una máquina distinta. La ubicación es configuración,
> no código. Decisión: [ADR-0005](../decisiones/ADR-0005-topologia-distribuida.md).

---

## 1. Nodos

| Nodo | Rol | Componentes |
|---|---|---|
| **Servidor** (mini-PC en casa, 24/7) | núcleo con estado | `recorder`, `store`, `runner` en vivo (motor + alertas), **`execution`** (sólo acá), `api`, `query`, `journal`, `notify` |
| **Estación** (PC, con la RTX 5050) | cliente + cómputo pesado | cliente de escritorio, `worker` (backtests, datasets, entrenamiento), `llm-gateway` grande (9B / MoE 35B) |
| **Teléfono** (S25, Android) | cliente + LLM local | cliente Android, LLM 4B en el dispositivo, clave de firma en Keystore |

Si la GPU está en el servidor y no en la PC, `worker` y `llm-gateway` se mueven ahí
cambiando sólo el perfil del nodo.

## 2. Reglas

1. **Contratos, no ubicaciones**: los componentes se hablan sólo a través de APIs
   definidas en el IDL (`schema/`). La misma interfaz tiene dos transportes: en proceso
   (si dos componentes conviven en un binario) y en red.
2. **Transporte de red**: Connect-RPC/gRPC sobre Tailscale. Los streams se reanudan por
   cursor (`ingest_seq`), así una desconexión no pierde ni duplica eventos.
3. **Singletons con lease**: `execution` y el `runner` en vivo son instancias únicas. Lease
   en Postgres; quien lo pierde deja de actuar de inmediato. `execution` sólo existe en el
   perfil "servidor" (allí están la IP autorizada y las claves).
4. **Workers sin estado**: toman trabajos de una cola en Postgres, leen datos y escriben
   resultados con manifiesto. Pueden correr en la PC o en el servidor.
5. **Datos para cómputo pesado**: los segmentos Parquet son direccionables por contenido
   (hash). La PC **sincroniza** los segmentos que necesita y verifica los hashes; no
   entrena leyendo por red fila por fila.
6. **Versionado de protocolo**: cada servicio publica su versión; los clientes aceptan
   ±1 versión menor y avisan si están desactualizados.
7. **Reloj**: sólo el servidor estampa `ts_available`, con NTP/chrony. Los clientes no
   producen marcas de tiempo autoritativas. Los comandos firmados llevan la hora del
   dispositivo con una tolerancia (default ±30 s) para evitar replays.

## 3. Red y permisos (Tailscale ACL)

```
teléfono, PC  ──► api (443)            sí
teléfono, PC  ──► query (lectura)       sí (vía api)
llm-gateway   ──► query, journal-notes  sí
llm-gateway   ──► api:/commands         NO
cualquiera    ──► execution             NO (sólo api, dentro del servidor)
worker        ──► store (lectura), cola sí
execution     ──► exchanges (salida)    sí, por IP fija o VPS de salida
```

## 4. Comportamiento sin conexión

- Los clientes cachean lo último que vieron y lo muestran con una franja "desactualizado
  desde HH:MM".
- **Los comandos de trading nunca se encolan sin conexión.** Una orden que se ejecuta
  diez minutos después, con otro precio, es peor que una orden rechazada. Sin conexión con
  `execution`, el ticket se deshabilita.
- Las posiciones siguen protegidas por stops nativos (ver doc 06).

## 5. Android

- Las restricciones de segundo plano de Android impiden mantener conexiones largas: las
  alertas llegan por push. Opciones: UnifiedPush con servidor `ntfy` propio (sin Google),
  FCM, y Telegram como respaldo. Se decide en el spike de M3.
- El LLM corre sólo en primer plano, en un proceso aislado dentro de la app (ver doc 08).
- La clave de firma de comandos vive en el Android Keystore (StrongBox si está disponible)
  y exige biometría **por operación**.

## 6. Perfiles de despliegue

`deploy/profiles/{server,workstation,phone}.toml` declaran qué componentes corren en cada
nodo y a qué direcciones se conectan. Un nodo de desarrollo puede correr **todo** en una
sola máquina (perfil `dev-all`) con el exchange en modo paper.
