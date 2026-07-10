import { beforeEach, expect, test } from "vitest";

import { useApp } from "../stores/app";

beforeEach(() => {
  useApp.setState({
    view: "home",
    service: "vl16",
    tasks: [],
    taskRevision: 0,
    taskRevisions: {},
    taskFieldRevisions: {},
    taskSnapshotRequest: 0,
    taskSnapshotApplied: 0,
    selectedTaskId: null,
  });
});

test("selects the task prepared for the result viewer", () => {
  useApp.getState().setSelectedTaskId("task-1");

  expect(useApp.getState().selectedTaskId).toBe("task-1");
});

test("lets a snapshot repair state when no event arrived during the request", () => {
  useApp.getState().upsertTask({ id: "task-1", status: "pending" });
  const snapshot = useApp.getState().beginTaskSnapshot();

  useApp.getState().mergeTasks(
    [
      {
        id: "task-1",
        status: "done",
        input_path: "C:/docs/task-1.png",
        created_at: 1,
      },
    ],
    snapshot,
  );

  expect(useApp.getState().tasks).toEqual([
    {
      id: "task-1",
      status: "done",
      input_path: "C:/docs/task-1.png",
      created_at: 1,
    },
  ]);
});

test("does not let a late snapshot revert an event received during its request", () => {
  const snapshot = useApp.getState().beginTaskSnapshot();
  useApp.getState().upsertTask({ id: "task-1", status: "done" });

  expect(useApp.getState().taskRevision).toBe(snapshot.baseRevision + 1);
  useApp.getState().mergeTasks(
    [
      {
        id: "task-1",
        status: "pending",
        input_path: "C:/docs/task-1.png",
        created_at: 1,
      },
    ],
    snapshot,
  );

  expect(useApp.getState().tasks).toEqual([
    {
      id: "task-1",
      status: "done",
      input_path: "C:/docs/task-1.png",
      created_at: 1,
    },
  ]);
});

test("fills snapshot metadata for a new id without losing its concurrent event", () => {
  const snapshot = useApp.getState().beginTaskSnapshot();
  useApp.getState().upsertTask({
    id: "new-task",
    status: "processing",
    progress_page: 1,
    total_pages: 2,
  });

  useApp.getState().mergeTasks(
    [
      {
        id: "new-task",
        service: "vl16",
        status: "pending",
        input_path: "C:/docs/new-task.png",
        created_at: 2,
      },
    ],
    snapshot,
  );

  expect(useApp.getState().tasks).toEqual([
    {
      id: "new-task",
      service: "vl16",
      status: "processing",
      input_path: "C:/docs/new-task.png",
      progress_page: 1,
      total_pages: 2,
      created_at: 2,
    },
  ]);
});

test("ignores an older snapshot after a newer snapshot has been applied", () => {
  const older = useApp.getState().beginTaskSnapshot();
  const newer = useApp.getState().beginTaskSnapshot();

  useApp.getState().mergeTasks(
    [
      {
        id: "new-task",
        status: "pending",
        input_path: "C:/docs/new-task.png",
      },
    ],
    newer,
  );
  useApp.getState().mergeTasks([], older);

  expect(useApp.getState().tasks).toEqual([
    {
      id: "new-task",
      status: "pending",
      input_path: "C:/docs/new-task.png",
    },
  ]);
  expect(useApp.getState().taskSnapshotApplied).toBe(newer.requestId);
});

test("allows an older snapshot when a newer request never applied", () => {
  const older = useApp.getState().beginTaskSnapshot();
  useApp.getState().beginTaskSnapshot();

  useApp.getState().mergeTasks(
    [{ id: "available", status: "done" }],
    older,
  );

  expect(useApp.getState().tasks).toEqual([
    { id: "available", status: "done" },
  ]);
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
