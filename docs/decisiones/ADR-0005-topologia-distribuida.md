# ADR-0005 — Componentes desplegables en máquinas distintas

- Estado: **aceptada**
- Fecha: 2026-10-27
- Diseño: [07-topologia-distribuida.md](../arquitectura/07-topologia-distribuida.md)

## Contexto

Hay tres máquinas (mini-PC en casa, PC con la GPU y teléfono Android) con capacidades y
disponibilidad distintas. El plan v2 suponía todo el backend en la mini-PC y no decía
dónde está la GPU (puede estar en la PC).

## Decisión

1. Interfaces definidas en el IDL con dos transportes intercambiables: en proceso y en red
   (Connect-RPC/gRPC sobre Tailscale), con streams reanudables por cursor.
2. `execution` es un singleton con lease, desplegable **sólo** en el perfil servidor.
3. Workers sin estado (backtest, dataset, entrenamiento) con cola en Postgres, ejecutables
   en la PC o en el servidor.
4. Segmentos de datos direccionables por contenido y sincronizables con verificación de
   hash.
5. Perfiles de despliegue declarativos por nodo, incluido `dev-all` (todo en una
   máquina, en paper).
6. Los comandos de trading nunca se encolan sin conexión.

## Alternativas descartadas

- **Broker de mensajes (NATS, Kafka, Redis Streams) desde el día uno**: agrega operación
  sin una necesidad demostrada para un solo usuario. Se reconsidera si aparecen más de
  dos consumidores remotos por stream o si la reanudación por cursor resulta
  insuficiente.
- **Monolito sin fronteras de red**: impide mover el cómputo a la GPU de la PC.
- **`execution` en la PC o en el teléfono**: no tienen la IP autorizada ni la
  disponibilidad 24/7, y multiplicarían la superficie de las claves.
- **Cola de órdenes offline en el teléfono**: ejecutaría órdenes con precios viejos.

## Consecuencias

- Cada interfaz necesita tests de contrato y de compatibilidad de versión.
- Se paga un costo de serialización incluso en proceso cuando conviene probar el camino de
  red; los tests de integración corren ambos transportes.
