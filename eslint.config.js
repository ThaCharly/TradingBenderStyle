import js from "@eslint/js";
import globals from "globals";
import tseslint from "typescript-eslint";
import { architectureRules } from "./tools/arch-tests/eslint-rules.js";

export default tseslint.config(
  { ignores: ["**/dist/**", "**/node_modules/**", "**/src/gen/**", "target/**", "spikes/**"] },
  js.configs.recommended,
  ...tseslint.configs.strict,
  {
    languageOptions: { globals: { ...globals.browser } },
  },
  {
    files: ["tools/**/*.js"],
    languageOptions: { globals: { ...globals.node } },
  },
  ...architectureRules,
);
