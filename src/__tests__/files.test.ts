import { expect, test } from "vitest";

import { filterSupported } from "../lib/files";

test("filters supported OCR paths case-insensitively without reordering", () => {
  expect(
    filterSupported([
      "C:/docs/first.PNG",
      "C:/docs/skip.txt",
      "C:/docs/second.jpg",
      "C:/docs/third.JPEG",
      "C:/docs/fourth.WebP",
      "C:/docs/fifth.PDF",
      "C:/docs/no-extension",
    ]),
  ).toEqual([
    "C:/docs/first.PNG",
    "C:/docs/second.jpg",
    "C:/docs/third.JPEG",
    "C:/docs/fourth.WebP",
    "C:/docs/fifth.PDF",
  ]);
});
