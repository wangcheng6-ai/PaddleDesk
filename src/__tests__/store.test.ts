import { beforeEach, expect, test } from "vitest";

import { useApp } from "../stores/app";

beforeEach(() => {
  useApp.setState({ view: "home", service: "vl16", tasks: [] });
});

test("upsertTask merges updates for the same task id", () => {
  useApp.getState().upsertTask({
    id: "task-1",
    status: "processing",
    progress_page: 2,
    total_pages: 5,
  });
  useApp.getState().upsertTask({ id: "task-1", status: "done" });

  expect(useApp.getState().tasks).toEqual([
    {
      id: "task-1",
      status: "done",
      progress_page: 2,
      total_pages: 5,
    },
  ]);
});
