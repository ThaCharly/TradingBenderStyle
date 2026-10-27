// Reglas de arquitectura para TypeScript (#20). Las incluye eslint.config.js de la raíz.
//
// ADR-0006 / ADR-0013, capa 1: el módulo LLM del cliente no importa el cliente de
// comandos. Paquetes LLM: `apps/client/packages/llm*` y cualquier carpeta `llm/` de la app
// de escritorio/Android (`apps/shell`).

import { fileURLToPath } from "node:url";

// ESLint busca la configuración desde la carpeta de cada archivo, y los patrones de `files`
// son relativos a la configuración que encuentra (la de cada paquete). Con `basePath` en la
// raíz del repo, los patrones valen igual desde cualquier paquete que reexporte la raíz.
const REPO_ROOT = fileURLToPath(new URL("../../", import.meta.url));

export const LLM_FILES = ["apps/client/packages/llm*/**", "apps/shell/**/llm/**"];

const COMMANDS_CLIENT = {
  group: ["@bender/commands-client", "@bender/commands-client/*", "**/commands-client", "**/commands-client/**"],
  message:
    "ADR-0006: el módulo LLM no puede importar el cliente de comandos (docs/AGENTES.md).",
};

export const architectureRules = [
  {
    basePath: REPO_ROOT,
    files: LLM_FILES,
    rules: {
      "no-restricted-imports": ["error", { patterns: [COMMANDS_CLIENT] }],
    },
  },
];
