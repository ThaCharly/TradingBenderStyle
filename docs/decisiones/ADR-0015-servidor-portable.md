# ADR-0015 — Servidor portable: GCP Santiago por ahora, migrable a la mini-PC

- Estado: **aceptada**
- Fecha: 2026-10-28
- Afecta: [ADR-0005](ADR-0005-topologia-distribuida.md) (topología),
  [ADR-0010](ADR-0010-archivado-y-paso-a-produccion-del-grabador.md) §2 (gate de producción),
  PLAN §1 y §2.1

## Contexto

El usuario tiene unos 190 USD de créditos de Google Cloud y prefiere alojar ahí el
servidor 24/7, en la región de **Santiago de Chile** (`southamerica-west1`). Cuando se
terminen los créditos puede volver a la mini-PC en casa. Requisito explícito: **la
aplicación no puede depender de dónde está alojada**; migrar tiene que ser sencillo.

## Decisión

### 1. El servidor es un rol, no un lugar

"Servidor" es el perfil de despliegue `server` (doc 07), sea una VM de GCP, la mini-PC u
otra máquina Linux. El código no sabe ni le importa dónde corre.

### 2. Reglas de portabilidad (obligatorias)

1. **Sin servicios administrados del proveedor.** Nada de Cloud SQL, Cloud Storage como
   dependencia, Pub/Sub, Secret Manager, logging propietario ni balanceadores. Postgres,
   `ntfy`, el grabador y todo lo demás corren **dentro de la máquina**.
2. **Despliegue en contenedores** (imágenes OCI + `compose`) con la configuración en
   `deploy/`. Lo mismo corre en la VM y en la mini-PC. Lo único que cambia por host es un
   archivo de entorno: rutas, IP pública esperada y nombre de Tailscale.
3. **Todo el estado en un solo directorio de datos** (`BENDER_DATA_DIR`): segmentos,
   Postgres, keystore cifrado, configuración. Migrar = copiar ese directorio verificado.
4. **Red solamente por Tailscale.** Ningún puerto abierto en el firewall del proveedor,
   salvo el de Tailscale. Los clientes se conectan por el nombre de Tailscale, no por la IP
   de la nube.
5. **Destinos de archivo genéricos** (ADR-0010): si algún día se usa almacenamiento de
   objetos, va por la interfaz compatible con S3, nunca por la API propia del proveedor.
6. **Herramienta de migración** (`bender-migrate`): exporta el directorio de datos con
   hashes, lo importa en el destino, verifica, y deja registrado el `Gap` del cambio de
   host (ADR-0011).

### 3. GCP Santiago, configuración inicial

- VM chica de uso general (orden de magnitud: 2 vCPU, 4 GB de RAM), disco persistente de
  50–100 GB e **IP externa reservada**, que es la que va en la whitelist de los exchanges.
- Costo aproximado: unos 25–45 USD por mes entre VM, disco, IP y tráfico de salida. Con
  190 USD alcanza para unos 4–7 meses. Son cifras de referencia: hay que confirmarlas en
  la calculadora de GCP para `southamerica-west1`.
- **Si los créditos son de la prueba gratuita, vencen** (en general a los 90 días de
  activada la cuenta). La fecha de vencimiento define cuándo hay que tener lista la
  migración. Hay que confirmarla en la consola de facturación.
- Alerta de presupuesto en GCP al 50%, 80% y 100% de los créditos.

### 4. Lo que cambia en un VPS respecto de la casa

- **Ventajas**: está prendido de verdad las 24 horas, sin cortes de luz ni de internet
  domésticos, y trae IP fija.
- **Claves de los exchanges** en una máquina de un tercero. Se mitiga con claves sin
  permiso de retiro, whitelist de la IP reservada, disco cifrado y el keystore
  desbloqueado sólo en memoria (Pilar 3). El respaldo del keystore sigue siendo
  obligatorio antes de operar con plata real.
- **Región**: Chile no está entre los países que bloquean Binance o Bybit (verificar al
  habilitar cada exchange, junto con su política sobre IPs de centros de datos). La
  latencia a los servidores de los exchanges (~200–300 ms) no importa para este uso.
- La GPU y el LLM grande siguen en la PC.

## Alternativas descartadas

- **Usar servicios administrados de GCP para ahorrar trabajo** (Cloud SQL, Cloud Storage):
  atan la aplicación al proveedor y la migración deja de ser copiar un directorio.
- **Kubernetes (GKE)**: costo y complejidad desproporcionados para un solo usuario.
- **Esperar a la mini-PC**: los créditos ya están y el VPS resuelve la disponibilidad 24/7.

## Consecuencias

- El gate de producción del grabador (#140) pasa a ser "paso al servidor de producción",
  sea cual sea.
- Las tareas de infraestructura doméstica (UPS, IP del proveedor de internet) sólo aplican
  cuando el servidor sea la mini-PC.
- Un **simulacro de migración** (de la PC de desarrollo a la VM, y de ida y vuelta) es
  parte del gate.
