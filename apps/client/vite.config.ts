import { configDefaults, defineConfig } from "vitest/config";

export default defineConfig({
  server: { port: 5173, strictPort: true },
  // Los paquetes anidados corren sus propios tests.
  test: { exclude: [...configDefaults.exclude, "packages/**"] },
});
