import { test as base, expect } from "@playwright/test";
import fs from "fs";
import path from "path";

// Auto-collect Istanbul coverage from window.__coverage__ after each test
// when COVERAGE=1. Works with vite-plugin-istanbul (requireEnv: false).
base.afterEach(async ({ page }, testInfo) => {
  if (process.env.COVERAGE) {
    try {
      const coverage = await page.evaluate(() => (window as unknown as { __coverage__?: unknown }).__coverage__);
      if (coverage) {
        const dir = path.join(process.cwd(), "coverage", "raw");
        fs.mkdirSync(dir, { recursive: true });
        const file = path.join(dir, `${testInfo.testId}.json`);
        fs.writeFileSync(file, JSON.stringify(coverage));
      }
    } catch {}
  }
});

export const test = base;
export { expect };
