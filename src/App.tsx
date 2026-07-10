import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { Sidebar } from "./components/Sidebar";
import { TopBar } from "./components/TopBar";
import { Onboarding } from "./components/Onboarding";
import { UpdatePrompt } from "./components/UpdatePrompt";
import {
  createTaskFromClipboard,
  getSettings,
  getUsage,
  onCaptureDone,
  onQueueEvent,
  onUsageUpdated,
  setSettings,
} from "./lib/ipc";
import { useApp, type View } from "./stores/app";
import { Home } from "./views/Home";
import { Queue } from "./views/Queue";
import { Settings } from "./views/Settings";
import { Usage } from "./views/Usage";
import { Viewer } from "./views/Viewer";

const titleKeys: Record<View, string> = {
  home: "viewTitles.home",
  viewer: "viewTitles.viewer",
  queue: "viewTitles.queue",
  usage: "viewTitles.usage",
  settings: "viewTitles.settings",
};

function App() {
  const { t } = useTranslation();
  const view = useApp((state) => state.view);
  const service = useApp((state) => state.service);
  const setView = useApp((state) => state.setView);
  const setService = useApp((state) => state.setService);
  const setSelectedTaskId = useApp((state) => state.setSelectedTaskId);
  const upsertTask = useApp((state) => state.upsertTask);
  const setAutoOpenTaskId = useApp((state) => state.setAutoOpenTaskId);
  const setTodayPages = useApp((state) => state.setTodayPages);
  const [subscriptionReady, setSubscriptionReady] = useState(false);
  const [subscriptionFailed, setSubscriptionFailed] = useState(false);
  const [subscriptionAttempt, setSubscriptionAttempt] = useState(0);
  const [desktopActionFailed, setDesktopActionFailed] = useState(false);
  const [completionOpenFailed, setCompletionOpenFailed] = useState(false);
  const [onboardingOpen, setOnboardingOpen] = useState<boolean | null>(null);
  const [completion, setCompletion] = useState<{
    taskId: string;
    batchSize: number;
    service?: typeof service;
  } | null>(null);

  useEffect(() => {
    let active = true;
    void getSettings().then(
      (settings) => {
        if (active) {
          setOnboardingOpen(settings.onboarding_complete !== "1");
          const current = settings.current_service;
          if (
            current === "vl16" ||
            current === "pp_ocr_v6" ||
            current === "structure_v3"
          ) {
            setService(current);
          }
        }
      },
      () => {
        if (active) setOnboardingOpen(true);
      },
    );
    return () => {
      active = false;
    };
  }, [setService]);

  useEffect(() => {
    let disposed = false;
    let cleanup: (() => void) | undefined;
    let usageRequest = 0;
    const unlisteners: Array<() => void> = [];

    const refreshUsage = async () => {
      const request = ++usageRequest;
      try {
        const rows = await getUsage(1);
        if (disposed || request !== usageRequest) return;
        const pages = { vl16: 0, pp_ocr_v6: 0, structure_v3: 0 };
        rows.forEach((row) => {
          pages[row.service] += row.pages;
        });
        setTodayPages(pages);
        setSubscriptionFailed(false);
      } catch {
        if (!disposed && request === usageRequest) setSubscriptionFailed(true);
      }
    };

    void (async () => {
      unlisteners.push(
        await onQueueEvent((update) => {
          upsertTask(update);
          const current = useApp.getState();
          const task = current.tasks.find(({ id }) => id === update.id);
          const terminal = ["done", "failed", "canceled"].includes(
            update.status ?? "",
          );
          if (task?.batch_id && terminal) {
            const batch = current.tasks.filter(
              ({ batch_id }) => batch_id === task.batch_id,
            );
            const batchFinished = batch.every(({ status }) =>
              ["done", "failed", "canceled"].includes(status ?? ""),
            );
            if (batch.length > 1 && batchFinished) {
              const result = [...batch]
                .reverse()
                .find(({ status }) => status === "done");
              if (result) {
                setCompletion({
                  taskId: result.id,
                  batchSize: batch.length,
                  service: result.service,
                });
              }
              return;
            }
          }
          if (update.status !== "done") return;
          if (
            current.autoOpenTaskId === update.id &&
            task?.service === current.service &&
            (current.view === "home" || current.view === "queue")
          ) {
            current.setSelectedTaskId(update.id);
            current.setAutoOpenTaskId(null);
            current.setView("viewer");
          } else {
            if (current.autoOpenTaskId === update.id) {
              current.setAutoOpenTaskId(null);
            }
            setCompletion({
              taskId: update.id,
              batchSize: 1,
              service: task?.service,
            });
          }
        }),
      );
      unlisteners.push(await onUsageUpdated(() => void refreshUsage()));
      unlisteners.push(
        await onCaptureDone((taskId) => {
          const current = useApp.getState();
          const task = current.tasks.find(({ id }) => id === taskId);
          if (
            document.visibilityState === "visible" &&
            (!task?.service || task.service === current.service)
          ) {
            current.setSelectedTaskId(taskId);
            current.setView("viewer");
          } else {
            setCompletion({ taskId, batchSize: 1, service: task?.service });
          }
        }),
      );
      if (disposed) {
        unlisteners.forEach((unlisten) => unlisten());
      } else {
        cleanup = () => unlisteners.forEach((unlisten) => unlisten());
        setSubscriptionReady(true);
        setSubscriptionFailed(false);
        void refreshUsage();
      }
    })().catch(() => {
      unlisteners.forEach((unlisten) => unlisten());
      if (!disposed) {
        setSubscriptionReady(false);
        setSubscriptionFailed(true);
      }
    });

    return () => {
      disposed = true;
      cleanup?.();
    };
  }, [setSelectedTaskId, setTodayPages, setView, subscriptionAttempt, upsertTask]);

  useEffect(() => {
    if (view !== "home") return;

    const pasteClipboardImage = (event: KeyboardEvent) => {
      if (!(event.ctrlKey || event.metaKey) || event.key.toLowerCase() !== "v") return;
      const target = event.target;
      if (
        target instanceof HTMLElement &&
        (target.isContentEditable || ["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName))
      ) {
        return;
      }
      event.preventDefault();
      void createTaskFromClipboard(service).then(
        (taskId) => {
          setDesktopActionFailed(false);
          setAutoOpenTaskId(taskId);
          setView("queue");
        },
        (error) => {
          setDesktopActionFailed(true);
          if (String(error).toLowerCase().includes("authentication")) {
            setView("settings");
          }
        },
      );
    };

    window.addEventListener("keydown", pasteClipboardImage);
    return () => window.removeEventListener("keydown", pasteClipboardImage);
  }, [service, setAutoOpenTaskId, setView, view]);

  const taskView = view === "home" || view === "queue";
  const content =
    taskView && !subscriptionReady ? (
      subscriptionFailed ? null : (
        <p role="status">{t("runtime.connectingQueueEvents")}</p>
      )
    ) : view === "home" ? (
      <Home />
    ) : view === "queue" ? (
      <Queue />
    ) : view === "viewer" ? (
      <Viewer />
    ) : view === "usage" ? (
      <Usage />
    ) : view === "settings" ? (
      <Settings onOpenOnboarding={() => setOnboardingOpen(true)} />
    ) : (
      <h1>{t(titleKeys[view])}</h1>
    );

  return (
    <div className="app-shell">
      <Sidebar />
      <section className="workspace">
        <TopBar />
        {subscriptionFailed && (
          <div className="runtime-alert" role="alert">
            <span>{t("runtime.queueEventsUnavailable")}</span>
            <button
              type="button"
              onClick={() => {
                setSubscriptionFailed(false);
                setSubscriptionAttempt((attempt) => attempt + 1);
              }}
            >
              {t("actions.retry")}
            </button>
          </div>
        )}
        {desktopActionFailed && (
          <div className="runtime-alert" role="alert">
            <span>{t("runtime.clipboardImageUnavailable")}</span>
            <button type="button" onClick={() => setDesktopActionFailed(false)}>
              {t("actions.dismiss")}
            </button>
          </div>
        )}
        {completionOpenFailed ? (
          <div className="runtime-alert" role="alert">
            <span>{t("runtime.openCompletedFailed")}</span>
            <button type="button" onClick={() => setCompletionOpenFailed(false)}>
              {t("actions.dismiss")}
            </button>
          </div>
        ) : null}
        {completion ? (
          <div className="runtime-alert" role="status">
            <span>
              {completion.batchSize > 1
                ? t("runtime.batchCompleted", { count: completion.batchSize })
                : t("runtime.taskCompleted")}
            </span>
            <button
              type="button"
              onClick={() =>
                void (async () => {
                  setCompletionOpenFailed(false);
                  if (completion.service && completion.service !== service) {
                    try {
                      await setSettings({ current_service: completion.service });
                      setService(completion.service);
                    } catch {
                      setCompletionOpenFailed(true);
                      return;
                    }
                  }
                  setSelectedTaskId(completion.taskId);
                  setCompletion(null);
                  setView("viewer");
                })()
              }
            >
              {t("actions.viewResult")}
            </button>
            <button type="button" onClick={() => setCompletion(null)}>
              {t("actions.dismiss")}
            </button>
          </div>
        ) : null}
        <main className="view-content">{content}</main>
      </section>
      <Onboarding open={onboardingOpen === true} onClose={() => setOnboardingOpen(false)} />
      {onboardingOpen === false ? <UpdatePrompt /> : null}
    </div>
  );
}

export default App;
