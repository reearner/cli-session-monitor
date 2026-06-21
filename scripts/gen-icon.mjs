// Rasterize src-tauri/icons/icon.svg -> icon-source.png (1024px), then run
//   npx @tauri-apps/cli@2 icon src-tauri/icons/icon-source.png
// to regenerate every platform icon. Dev-only; needs `@resvg/resvg-js`.
import { readFileSync, writeFileSync } from "node:fs";
import { Resvg } from "@resvg/resvg-js";

const svg = readFileSync("src-tauri/icons/icon.svg", "utf8");
const png = new Resvg(svg, { fitTo: { mode: "width", value: 1024 } })
  .render()
  .asPng();
writeFileSync("src-tauri/icons/icon-source.png", png);
console.log("wrote src-tauri/icons/icon-source.png (1024x1024)");
