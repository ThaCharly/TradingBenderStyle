import base from "../../eslint.config.js";

// Los paquetes anidados se lintean con su propio script.
export default [{ ignores: ["packages/**"] }, ...base];
