// Arma `dist/artifact.html` desde `dist/index.html`: el contenido de la página sin
// <!doctype>, <html>, <head> ni <body> (el publicador de Artifacts los agrega).
import { readFileSync, writeFileSync } from "node:fs";

const html = readFileSync(new URL("./dist/index.html", import.meta.url), "utf8");
const pick = (re) => (html.match(re) ?? [])[0] ?? "";
const title = pick(/<title>[\s\S]*?<\/title>/);
const styles = html.match(/<style[\s\S]*?<\/style>/g) ?? [];
const scripts = html.match(/<script[\s\S]*?<\/script>/g) ?? [];
const body = (html.match(/<body>([\s\S]*)<\/body>/) ?? [])[1] ?? "";
const bodyNoScripts = body.replace(/<script[\s\S]*?<\/script>/g, "");
const out = [
  title,
  '<meta name="claude:viewport-fit-sides" content="cover">',
  ...styles,
  bodyNoScripts.trim(),
  ...scripts,
].join("\n");
writeFileSync(new URL("./dist/artifact.html", import.meta.url), out);
console.log(`dist/artifact.html: ${(out.length / 1024).toFixed(0)} KB`);
