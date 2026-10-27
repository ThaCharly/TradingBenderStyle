// Fixture de violación de la regla TS de #20: con la configuración real de ESLint del
// repo, un archivo del módulo LLM que importa el cliente de comandos falla. Se usa la
// configuración de un paquete que reexporta la de la raíz (como chart-scene), que es lo
// que va a tener el paquete LLM cuando exista.

import { fileURLToPath } from "node:url";
import { ESLint } from "eslint";
import { describe, expect, it } from "vitest";

const root = fileURLToPath(new URL("../../", import.meta.url));
const eslint = new ESLint({
  cwd: root,
  overrideConfigFile: `${root}apps/client/packages/chart-scene/eslint.config.js`,
});

async function restrictedImports(code, file) {
  const [result] = await eslint.lintText(code, { filePath: `${root}${file}` });
  return result.messages.filter((m) => m.ruleId === "no-restricted-imports");
}

describe("ADR-0006: el módulo LLM no importa el cliente de comandos", () => {
  const imports = [
    'import { send } from "@bender/commands-client";',
    'import { sign } from "@bender/commands-client/sign";',
    'import { send } from "../../commands-client/src/index";',
    'export { send } from "@bender/commands-client";',
  ];

  for (const code of imports) {
    it(`falla en el paquete LLM: ${code}`, async () => {
      const found = await restrictedImports(code, "apps/client/packages/llm/src/tools.ts");
      expect(found).toHaveLength(1);
      expect(found[0].message).toContain("ADR-0006");
    });
  }

  it("también en la carpeta llm/ de la app nativa", async () => {
    const found = await restrictedImports(imports[0], "apps/shell/src/llm/broker.ts");
    expect(found).toHaveLength(1);
  });

  it("fuera del módulo LLM se permite", async () => {
    const found = await restrictedImports(imports[0], "apps/client/packages/ui/src/ticket.ts");
    expect(found).toEqual([]);
  });

  it("el módulo LLM puede importar otras cosas", async () => {
    const found = await restrictedImports(
      'import { DecimalSchema } from "@bender/types";',
      "apps/client/packages/llm/src/tools.ts",
    );
    expect(found).toEqual([]);
  });
});
