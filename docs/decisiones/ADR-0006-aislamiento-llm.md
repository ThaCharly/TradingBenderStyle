# ADR-0006 — El LLM sólo consulta y escribe notas propias; ningún camino a ejecución

- Estado: **aceptada**; ampliada por [ADR-0013](ADR-0013-mcp-de-analisis.md) (servidor MCP para clientes externos y escritura en la capa de IA)
- Fecha: 2026-10-27
- Diseño: [08-aislamiento-llm.md](../arquitectura/08-aislamiento-llm.md)

## Contexto

El LLM corre en el teléfono **dentro de la misma app que puede operar** y en la PC. Un
modelo puede ser manipulado por el contenido que lee (inyección de instrucciones) o
simplemente equivocarse. La prohibición tiene que ser estructural, no una instrucción en
el prompt.

## Decisión

Defensa en cinco capas, cada una suficiente por sí sola para cortar el camino:
1. **Código**: el gateway y el módulo LLM no dependen del cliente de comandos (test de
   grafo de dependencias en CI). Registro de herramientas cerrado con snapshot.
2. **Herramientas**: sólo consultas, más `append_assistant_note` (append-only, en su propio
   espacio, con límites de tamaño y tasa).
3. **Credenciales**: token sin alcance de comandos; `execution` exige firma de
   dispositivo con biometría por operación en Android.
4. **Red**: ACL de Tailscale y firewall; rol de base de datos de sólo lectura más
   `INSERT` en `assistant_notes`.
5. **Presentación**: la salida del LLM nunca se convierte en acciones; el LLM de Android
   corre en un proceso aislado detrás de un broker de herramientas.

## Alternativas descartadas

- **"Borradores de orden" sugeridos por el LLM que el usuario confirma**: abren un camino
  de datos LLM → ticket. Si el ticket viene pre-llenado, la confirmación se vuelve un
  trámite. Descartado.
- **Restricción sólo por prompt de sistema**: no es una garantía.
- **Permitir que el LLM cree alertas o dibujos**: no ejecutan, pero son entradas que el
  usuario usa para decidir y amplían la superficie. Fuera de alcance; reconsiderable con
  una ADR nueva.
- **Que el LLM edite sus propias notas**: append-only simplifica la auditoría.

## Consecuencias

- Seis pruebas obligatorias en CI (doc 08) bloquean cualquier regresión.
- Las funciones del asistente quedan limitadas a explicar, resumir y revisar. Es lo
  buscado.
