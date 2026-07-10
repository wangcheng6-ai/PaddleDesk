// @ts-expect-error Vitest runs on Node; the app intentionally omits Node types.
import { readFileSync } from "node:fs";
import { expect, test } from "vitest";

const styles = readFileSync("src/index.css", "utf8");

test("keeps the shell reachable below 800 by 600", () => {
  expect(styles).not.toMatch(/body\s*\{[^}]*min-width:\s*800px/s);
  expect(styles).not.toMatch(/body\s*\{[^}]*min-height:\s*600px/s);
  expect(styles).toMatch(/\.app-shell\s*\{[^}]*height:\s*100vh/s);
  expect(styles).toMatch(/\.sidebar\s*\{[^}]*overflow-y:\s*auto/s);
  expect(styles).toMatch(/\.topbar\s*\{[^}]*overflow-x:\s*auto/s);
  expect(styles).toMatch(/\.view-content\s*\{[^}]*overflow:\s*auto/s);
});

test("lets the drop registration alert wrap in a narrow window", () => {
  expect(styles).toMatch(
    /\.drop-zone-alert\s*\{[^}]*display:\s*flex[^}]*flex-wrap:\s*wrap/s,
  );
});
