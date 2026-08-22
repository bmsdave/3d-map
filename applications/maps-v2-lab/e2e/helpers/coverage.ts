import { test as base, expect } from "@playwright/test";
import fs from "fs";
import path from "path";

// Auto-collect Istanbul coverage from window.__coverage__ after each test
// when COVERAGE=1. Works with vite-plugin-istanbul (requireEnv: false).
export const test = base.extend<{ __coverage: void }>({
  __coverage: [async ({ page }, use, testInfo) => {
    await use();
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
  }, { auto: true }],
});

export { expect };
