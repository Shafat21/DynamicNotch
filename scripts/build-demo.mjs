// Builds the island frontend into docs/demo for the website: the real UI, with scripts/demo/tauri-mock.js
// standing in for Tauri. Run `npm run demo` after UI changes so the site shows the latest island.
import { build } from "vite";
import { copyFileSync, readFileSync, writeFileSync } from "node:fs";

const out = "docs/demo";
await build({
  configFile: false,
  base: "./",
  logLevel: "warn",
  build: { target: "chrome120", outDir: out, emptyOutDir: true, rollupOptions: { input: { main: "index.html" } } },
});
copyFileSync("scripts/demo/tauri-mock.js", `${out}/tauri-mock.js`);
const html = readFileSync(`${out}/index.html`, "utf8");
if (!html.includes("tauri-mock.js")) writeFileSync(`${out}/index.html`, html.replace("<head>", '<head>\n<script src="tauri-mock.js"></script>'));
console.log(`demo built in ${out}`);
