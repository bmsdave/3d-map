import { defineConfig } from "vite";
import istanbul from "vite-plugin-istanbul";

// The config runs in Node, but the lab has no `@types/node` and does not
// need one for a single environment variable.
declare const process: { env: Record<string, string | undefined> };

// 5178 by default, so the README's link and the Playwright configs keep
// working without anyone setting anything. `PORT` overrides it, because a
// second copy of the lab running beside the first — a preview alongside a
// test run — needs somewhere else to bind, and `strictPort` means falling
// back silently is not on offer.
const port = Number(process.env.PORT) || 5178;

// GitHub Pages serves a project site from "/<repo>/", not from the root,
// and the wasm bundle, the stylesheet and the tile package are all fetched
// by URL. `BASE_PATH` is how the Pages workflow says where the site lives;
// everything else — dev, preview, e2e — keeps the root it always had.
const base = process.env.BASE_PATH || "/";

// Rollup wants absolute entry paths, and this config has no `@types/node`
// to get them from `path`. A URL relative to the config's own module is
// the same answer without the dependency.
const page = (file: string): string => new URL(file, import.meta.url).pathname;

export default defineConfig({
  base,
  server: {
    port,
    strictPort: true,
    // Playwright writes traces, screenshots and coverage into the project
    // while the suite runs. The dev server watches the project, so those
    // writes reload every page it is serving — including the one a test is
    // mid-assertion on, which surfaces as "execution context was destroyed"
    // in whichever test happened to be running. None of these are sources.
    watch: {
      ignored: ["**/test-results/**", "**/playwright-report/**", "**/coverage/**", "**/dist/**"],
    },
  },
  preview: { port, strictPort: true },
  plugins: process.env.COVERAGE ? [istanbul({ include: "src/*", exclude: ["node_modules", "test", "e2e"], extension: [".js", ".ts"], requireEnv: false })] : [],
  build: {
    rollupOptions: {
      // Two pages out of one build: the lab at "/" and the public demo at
      // "/demo/". They share the SDK, so they share a build — a second
      // Vite project would mean a second wasm copy and a second chance for
      // the demo to drift from what the studies actually measure.
      input: {
        lab: page("index.html"),
        demo: page("demo/index.html"),
      },
    },
  },
});
