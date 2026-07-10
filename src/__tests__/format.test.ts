import { expect, test } from "vitest";

import { formatDate, formatNumber } from "../lib/format";

test("formats numbers and dates with the requested locale", () => {
  expect(formatNumber(1234, "en")).toBe("1,234");
  expect(formatDate(new Date(2026, 0, 2), "en")).toContain("2026");
});
