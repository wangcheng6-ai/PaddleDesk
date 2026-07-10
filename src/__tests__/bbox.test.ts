import { expect, test } from "vitest";

import { scaleBbox } from "../lib/bbox";

test("scales page coordinates to the rendered size", () => {
  expect(
    scaleBbox(
      [10, 20, 110, 220],
      { width: 200, height: 400 },
      { width: 100, height: 200 },
    ),
  ).toEqual({ left: 5, top: 10, width: 50, height: 100 });
});
