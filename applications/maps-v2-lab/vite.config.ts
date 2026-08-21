import { defineConfig } from "vite";

// The config runs in Node, but the lab has no `@types/node` and does not
// need one for a single environment variable.
declare const process: { env: Record<string, string | undefined> };

// 5178 by default, so the README's link and the Playwright configs keep
// working without anyone setting anything. `PORT` overrides it, because a
// second copy of the lab running beside the first — a preview alongside a
// test run — needs somewhere else to bind, and `strictPort` means falling
// back silently is not on offer.
const port = Number(process.env.PORT) || 5178;

export default defineConfig({
  server: { port, strictPort: true },
  preview: { port, strictPort: true },
});
