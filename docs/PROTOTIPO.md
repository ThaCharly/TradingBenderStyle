# Prototipo inicial (P0): "Bender ve el mercado con sus propios ojos"

Lo primero que se construye **no** es la gráfica bonita ni el trading. Es la tubería
completa más angosta posible que atraviesa las piezas que, si salen mal, rompen todo lo
demás: el modelo de datos, el núcleo temporal, el grabador y la gráfica.

Seguimiento: issue **#141** en GitHub (etiqueta `prototipo`).

---

## Qué vas a poder hacer al terminar P0

En tu PC, sin teléfono, sin exchange con claves, sin motor ni LLM:

1. Arrancás todo con un comando (perfil `dev-all`).
2. El grabador empieza a capturar **BTC y ETH de Binance** (spot y perpetuos): trades,
   libro, funding, mark price.
3. Abrís el navegador en `localhost` y ves **una gráfica 4H + 30m sincronizadas**,
   construida **con tus propios datos grabados**, actualizándose en vivo. Podés pasar a
   cualquier temporalidad, de 1 m a 1 año (las de segundos, sólo dentro de la ventana de
   trades crudos; ADR-0014).
4. Dibujás líneas horizontales, de tendencia y rectángulos, y quedan guardados.
5. **Desenchufás internet un minuto.** Al volver, el grabador se recupera solo y la
   gráfica muestra el hueco marcado, en vez de inventar velas.
6. **Máquina del tiempo**: elegís "ayer a las 14:32" y la gráfica muestra exactamente lo
   que había en ese instante. Eso incluye la vela de 4H a medio formar como estaba, y
   tus dibujos de ese momento, no los posteriores. Le das play y lo ves avanzar.
7. Un reporte compara las velas que armó Bender contra las velas oficiales de Binance.
   Tienen que coincidir, y si no, saber por qué.

## Qué demuestra

| Pregunta | Cómo la responde P0 |
|---|---|
| ¿El modelo de datos aguanta datos reales? | El grabador escribe Binance real con el esquema universal |
| ¿El núcleo temporal es reproducible? | Los 5 tests normativos pasan; la máquina del tiempo funciona |
| ¿El grabador se recupera de fallas? | Huecos marcados y reanudación automática |
| ¿Lightweight Charts sirve? | El spike mide los criterios de ADR-0001 y la gráfica se construye encima |
| ¿Cuánto disco consume de verdad? | El reporte de 24 h corrige las estimaciones de ADR-0009 |

Y deja corriendo el grabador desde temprano: **cada día grabado es historia que no se
puede descargar después**.

## Qué **no** incluye

Otros exchanges, backfill histórico, indicadores, perfiles de volumen, alertas, teléfono,
trading (ni paper), motor, estadística, LLM. Todo eso se apoya en lo que P0 valida.

## Issues que componen P0 (en orden de dependencias)

| Paso | Issues | Qué sale |
|---|---|---|
| 1. Cimientos | #2, #20, #17, #18, #19, #21 | repo, CI, esquema de datos, tipos, transporte |
| 2. Núcleo temporal | #22, #23, #24, #25, #26, #27, #28, #29, #30 | almacenamiento, reloj, velas, `as_of`, Runner, tests normativos |
| 3. Grabador | #32, #156 | Binance BTC/ETH grabándose; validación contra velas oficiales |
| 4. Gráfica | #44 (spike, puede ir en paralelo desde el paso 1), #46, #47, #48, #49, #54 | servicio de velas, escena, render, dibujo, máquina del tiempo |
| 5. Integración | #155 | `make dev`: todo con un comando |

Lo que puede ir en paralelo: el spike de gráfica (#44) arranca apenas existe el repo
(#2), con datos sintéticos, mientras se construye el núcleo.

## Después de P0

- **P1 — Grabador en producción** (ADR-0014): perfiles y ventanas rotativas (#43), volumen
  por banda (#174), liquidez por banda (#175), procedencia y términos (#176), archivado
  opcional (#139), calidad de datos (#42), specs de instrumentos (#33), resto de los
  exchanges (#34–#37), compresión (#177) y el **gate** de
  paso a la mini-PC (#140).
- Después siguen los milestones en el orden del [ROADMAP](ROADMAP.md): alertas, paper y
  spot, futuros, motor…
