// Bundles the runtime into dist/host.cjs, the one file EchoSpot runs with Node.
import { build } from "esbuild";

await build({
  entryPoints: ["src/host.js"],
  bundle: true,
  format: "cjs",
  platform: "node",
  target: ["node20"],
  outfile: "dist/host.cjs",
  minify: false,
  legalComments: "none",
  define: { "process.env.NODE_ENV": '"production"' },
});
console.log("dist/host.cjs built");
