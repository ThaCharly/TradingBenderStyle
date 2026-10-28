# Funcionalidades de TradingBenderStyle

Qué va a tener la aplicación, en lenguaje llano. Es el resumen para el dueño; el detalle
técnico está en el [PLAN](PLAN.md), las [ADRs](decisiones/README.md) y los issues.

**Mantenimiento:** la sesión de planificación actualiza este archivo con cada cambio de
alcance. La sesión de ejecución actualiza la columna **Estado** cuando una funcionalidad
llega a `main`.

Estados: **planificado** · **en construcción** · **hecho** · **diferido** (con su motivo).
"Cuándo" es el milestone del [ROADMAP](ROADMAP.md); P0 es el prototipo actual.

---

## Gráfica

| Funcionalidad | Cuándo | Estado |
|---|---|---|
| Gráfica con tus propios datos grabados, en vivo | P0 | en construcción |
| Temporalidades de 1 minuto a 1 año, incluidas 2m, 3m y 5m (segundos sólo con el perfil "al tick" o dentro de la ventana de trades) | P0 | en construcción |
| Layout 4H + 30m por defecto; 1, 2 o 4 gráficas sincronizadas; escala log y lineal | P0 | en construcción |
| Cortes de datos visibles, sin velas inventadas; datos recuperados marcados | P0 | en construcción |
| **Máquina del tiempo**: volver a cualquier instante y verlo tal como estaba, con replay | P0 | en construcción |
| Universo curado: sólo los pares que vos agregás, sin ruido | M3 | planificado |
| Volumen de todos los exchanges, spot y perpetuos por separado o agregados | M3 | planificado |
| Gráfica en el teléfono, con gestos táctiles | M3 | planificado |

## Dibujo e indicadores

| Funcionalidad | Cuándo | Estado |
|---|---|---|
| Horizontal, rayo, tendencia, rectángulo, canal paralelo, Fibonacci, medidor, texto, posición larga/corta | P0 | en construcción |
| Dibujos compartidos entre instrumentos del mismo par (BTCUSDT y BTCUSDC ven lo mismo) | P0 | en construcción |
| Perfil de volumen anclado (VAH / POC / VAL) y VWAP anclado | M3 | planificado |
| Indicadores clásicos, funding, open interest, liquidaciones, divergencia spot/perps | M3 | planificado |
| **Ondas de Elliott**: herramienta de conteo manual con validación de reglas, guías de Fibonacci y conteos alternativos | M14 (#179; sólo depende de la gráfica) | planificado |
| Ondas de Elliott automáticas: el motor propone conteos con puntaje, nunca uno solo | M14 (#135) | planificado |
| Heatmap de liquidez (con resúmenes por minuto) | M14 (grabación desde P1) | planificado |
| Lenguaje propio para crear tus indicadores | M14 | planificado |

## Alertas

| Funcionalidad | Cuándo | Estado |
|---|---|---|
| Alertas de precio, línea, zona, indicador y cierre de vela | M5 | planificado |
| **Llegan al teléfono como notificación de la app**; Telegram sólo de respaldo | M5 | planificado |
| Crear una alerta desde un dibujo con un click | M5 | planificado |
| Alertas por eventos del motor ("rechazo en un VAL de 4H") | M8 | planificado |
| Alertas de cuenta: margen, PnL diario, distancia a liquidación | M7 | planificado |

## Trading

| Funcionalidad | Cuándo | Estado |
|---|---|---|
| Exchanges: Binance primero; después BloFin, BingX y Bybit; MEXC si su API lo permite | M6, M7 | planificado |
| Paper trading con fills, comisiones, funding y liquidaciones realistas | M6, M7 | planificado |
| Comprar o vender poniendo una línea en la gráfica; arrastrarla para modificar la orden | M6 | planificado |
| Escaleras de órdenes y planes de acumulación | M6 | planificado |
| Ticket de futuros: ponés cuánto arriesgás y el tamaño se calcula solo; varios take profits, stop obligatorio, aviso del precio de liquidación, plantillas | M7 | planificado |
| Protección de posiciones: cada posición queda con stop en el exchange o se cierra | M7 | planificado |
| Límites que te ponés vos: pérdida diaria máxima, pausa después de una pérdida, máximo de trades | M7 | planificado |
| Botón rojo: cancelar todo o cerrar todo | M6 | planificado |
| Cada orden se firma desde tu dispositivo; en el teléfono, con tu huella | M4 | planificado |

## Motor de mercado y estadística

| Funcionalidad | Cuándo | Estado |
|---|---|---|
| Niveles con fundamento: perfiles, soportes y resistencias, sesiones, POC vírgenes, redondos, Fibonacci y tus propios niveles | M8 | planificado |
| Canales y rangos a tu manera (desde ahora hasta donde empieza el canal); canales inclinados aparte | M8 | diferido: las definiciones se cierran después de usar las herramientas |
| Eventos: toque, rechazo, aceptación, ruptura, retest, con calidad del evento | M8 | planificado |
| Confluencias por zona | M8 | planificado |
| **Mensaje que te explica** ("llegó al VAL de… primer rechazo… históricamente X% fue al POC…") | M9 | planificado |
| Probabilidades auditables: casos, período, N y validación, comparadas contra el azar y la geometría | M9 | planificado |
| Modelos chicos calibrados por mercado y por tipo de evento | M12 | planificado |
| Mercado tradicional (SPY, NASDAQ, petróleo, Brent) para entrenar | M13 | planificado; intradía pago a decidir |

## Diario e IA

| Funcionalidad | Cuándo | Estado |
|---|---|---|
| Diario automático: cada trade con su R, su contexto y la foto del motor en la entrada | M10 | planificado |
| Errores detectados solos (sin stop, revenge, sobre-tamaño, stop movido en contra…) | M10 | planificado |
| Estadísticas: qué setups te funcionan, curva de capital, cuánto mejoraste | M10 | planificado |
| LLM 4B en el teléfono y 9B/35B en la PC para charlar y revisar el diario, con herramientas | M11 | planificado |
| **MCP de análisis**: Claude y Codex ven tu análisis y escriben en una capa propia de la IA, sin tocar lo tuyo | M15 (base después de P0) | planificado |
| Analizar "como si fuera" una fecha pasada, sin que la IA pueda ver el futuro | M15 | planificado |

## Datos

| Funcionalidad | Cuándo | Estado |
|---|---|---|
| Grabador en vivo de Binance (BTC y ETH, spot y perpetuos) | P0 | en construcción |
| Grabador del resto de los exchanges | P1 | planificado |
| Recuperación automática de cortes, sin ocultar huecos | P0 | en construcción |
| Perfil liviano por defecto (1m + resúmenes, trades 14 días) y perfil "al tick" para un servidor grande | P1 | planificado |
| Historia de Binance y Bybit descargada por vos desde la fuente oficial | M2 | planificado |
| Cada dato sabe su fuente y sus términos de uso (puerta abierta a acuerdos con exchanges) | P1 | planificado |

## Infraestructura

| Funcionalidad | Cuándo | Estado |
|---|---|---|
| Servidor 24/7 portable: hoy VM de GCP en Santiago, mañana la mini-PC, sin depender del proveedor | M4 | planificado |
| Migración de servidor con una herramienta y un simulacro probado | M4 (#181) | planificado |
| PC para desarrollo, cómputo pesado y LLM grande | — | definido |
| Acceso remoto privado (Tailscale), nada expuesto a internet | M4 | planificado |
| Respaldo de datos | — | diferido; obligatorio antes de operar con plata real |

## Fuera de alcance por ahora

- Datos del mercado tradicional en vivo.
- Modo producto, acuerdos de datos con exchanges, Bitunix: puerta abierta, nada construido.
