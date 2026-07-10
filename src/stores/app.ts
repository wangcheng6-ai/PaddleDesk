import { create } from "zustand";

export type View =
  | "home"
  | "viewer"
  | "queue"
  | "usage"
  | "settings";

export type ServiceId = "vl16" | "pp_ocr_v6" | "structure_v3";

export interface TaskSummary {
  id: string;
  service?: ServiceId;
  status?: string;
  input_path?: string;
  options_json?: string;
  progress_page?: number;
  total_pages?: number;
  error_kind?: string | null;
  error_msg?: string | null;
  created_at?: number;
  batch_id?: string | null;
}

type TaskField = Exclude<keyof TaskSummary, "id">;
type TaskFieldRevisions = Partial<Record<TaskField, number>>;

export interface TaskSnapshot {
  baseRevision: number;
  requestId: number;
}

export interface AppState {
  view: View;
  setView: (view: View) => void;
  service: ServiceId;
  setService: (service: ServiceId) => void;
  todayPages: Record<ServiceId, number>;
  setTodayPages: (pages: Record<ServiceId, number>) => void;
  tasks: TaskSummary[];
  taskRevision: number;
  taskRevisions: Record<string, number>;
  taskFieldRevisions: Record<string, TaskFieldRevisions>;
  taskSnapshotRequest: number;
  taskSnapshotApplied: number;
  beginTaskSnapshot: () => TaskSnapshot;
  upsertTask: (task: TaskSummary) => void;
  mergeTasks: (tasks: TaskSummary[], snapshot: TaskSnapshot) => void;
  removeTask: (id: string) => void;
  selectedTaskId: string | null;
  setSelectedTaskId: (id: string | null) => void;
  autoOpenTaskId: string | null;
  setAutoOpenTaskId: (id: string | null) => void;
}

export const useApp = create<AppState>((set, get) => ({
  view: "home",
  setView: (view) => set({ view }),
  service: "vl16",
  setService: (service) => set({ service }),
  todayPages: { vl16: 0, pp_ocr_v6: 0, structure_v3: 0 },
  setTodayPages: (todayPages) => set({ todayPages }),
  tasks: [],
  taskRevision: 0,
  taskRevisions: {},
  taskFieldRevisions: {},
  taskSnapshotRequest: 0,
  taskSnapshotApplied: 0,
  beginTaskSnapshot: () => {
    const state = get();
    const snapshot = {
      baseRevision: state.taskRevision,
      requestId: state.taskSnapshotRequest + 1,
    };
    set({ taskSnapshotRequest: snapshot.requestId });
    return snapshot;
  },
  upsertTask: (task) =>
    set((state) => {
      const revision = state.taskRevision + 1;
      const fieldRevisions = {
        ...state.taskFieldRevisions[task.id],
      };
      for (const field of Object.keys(task) as (keyof TaskSummary)[]) {
        if (field !== "id") {
          fieldRevisions[field] = revision;
        }
      }
      const index = state.tasks.findIndex(({ id }) => id === task.id);
      if (index === -1) {
        return {
          tasks: [...state.tasks, task],
          taskRevision: revision,
          taskRevisions: { ...state.taskRevisions, [task.id]: revision },
          taskFieldRevisions: {
            ...state.taskFieldRevisions,
            [task.id]: fieldRevisions,
          },
        };
      }

      const tasks = [...state.tasks];
      tasks[index] = { ...tasks[index], ...task };
      return {
        tasks,
        taskRevision: revision,
        taskRevisions: { ...state.taskRevisions, [task.id]: revision },
        taskFieldRevisions: {
          ...state.taskFieldRevisions,
          [task.id]: fieldRevisions,
        },
      };
    }),
  mergeTasks: (incoming, snapshot) =>
    set((state) => {
      if (snapshot.requestId < state.taskSnapshotApplied) return state;

      const current = new Map(state.tasks.map((task) => [task.id, task]));
      const tasks = incoming.map((task) => {
        const merged = { ...task };
        const currentTask = current.get(task.id);
        const revisions = state.taskFieldRevisions[task.id];

        if (currentTask && revisions) {
          for (const field of Object.keys(revisions) as TaskField[]) {
            if ((revisions[field] ?? 0) > snapshot.baseRevision) {
              Object.assign(merged, { [field]: currentTask[field] });
            }
          }
        }

        return merged;
      });
      const incomingIds = new Set(incoming.map(({ id }) => id));
      return {
        tasks: [
          ...tasks,
          ...state.tasks.filter(
            ({ id }) =>
              !incomingIds.has(id) &&
              (state.taskRevisions[id] ?? 0) > snapshot.baseRevision,
          ),
        ],
        taskSnapshotApplied: snapshot.requestId,
      };
    }),
  removeTask: (id) =>
    set((state) => ({ tasks: state.tasks.filter((task) => task.id !== id) })),
  selectedTaskId: null,
  setSelectedTaskId: (selectedTaskId) => set({ selectedTaskId }),
  autoOpenTaskId: null,
  setAutoOpenTaskId: (autoOpenTaskId) => set({ autoOpenTaskId }),
}));
