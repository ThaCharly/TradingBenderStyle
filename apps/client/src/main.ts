import { appTitle } from "./app";

const root = document.querySelector<HTMLDivElement>("#app");
if (root) {
  root.textContent = appTitle();
}
