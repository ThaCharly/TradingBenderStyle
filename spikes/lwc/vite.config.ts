import { defineConfig } from "vite";
import { viteSingleFile } from "vite-plugin-singlefile";

// Un solo HTML con todo adentro: se abre en el teléfono sin servidor.
export default defineConfig({ plugins: [viteSingleFile()] });
