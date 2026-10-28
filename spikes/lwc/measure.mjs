// Corre la medición del spike en Chromium (headless) y guarda resultado y capturas.
// Uso: node measure.mjs [--headed]
import { createRequire } from "node:module";
import { mkdirSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

// Playwright viene instalado globalmente (NODE_PATH) o en el proyecto.
const { chromium } = createRequire(import.meta.url)("playwright");

const html = fileURLToPath(new URL("./dist/index.html", import.meta.url));
const out = fileURLToPath(new URL("./resultados/", import.meta.url));
mkdirSync(out, { recursive: true });
const browser = await chromium.launch({ headless: !process.argv.includes("--headed") });
const page = await browser.newPage({ viewport: { width: 1600, height: 1000 }, deviceScaleFactor: 1 });
page.on("pageerror", (e) => console.error("error en la página:", e.message));
await page.goto(`file://${html}`);
await page.waitForFunction(() => document.getElementById("status")?.textContent?.includes("cargadas"));
await page.screenshot({ path: `${out}/inicial.png` });
await page.click("#heat");
await page.waitForTimeout(300);
await page.screenshot({ path: `${out}/heatmap.png` });
await page.click("#heat");
await page.click("#run");
await page.waitForFunction(() => !document.getElementById("run").disabled, null, { timeout: 120_000 });
const text = await page.textContent("#results");
console.log(text);
writeFileSync(`${out}/headless.txt`, text + "\n");
await page.screenshot({ path: `${out}/despues.png`, fullPage: true });
await browser.close();
