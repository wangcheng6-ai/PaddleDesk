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
} from "./lib/ipc";
import { useApp, type View } from "./stores/app";
import { Home } from "./views/Home";
import { History } from "./views/History";
import { Queue } from "./views/Queue";
import { Settings } from "./views/Settings";
import { Usage } from "./views/Usage";
import { Viewer } from "./views/Viewer";

const titleKeys: Record<View, string> = {
  home: "viewTitles.home",
  viewer: "viewTitles.viewer",
  queue: "viewTitles.queue",
  history: "viewTitles.history",
  usage: "viewTitles.usage",
  settings: "viewTitles.settings",
};

function App() {
  const { t } = useTranslation();
  const view = useApp((state) => state.view);
  const service = useApp((state) => state.service);
  const setView = useApp((state) => state.setView);
  const setSelectedTaskId = useApp((state) => state.setSelectedTaskId);
  const upsertTask = useApp((state) => state.upsertTask);
  const setTodayPages = useApp((state) => state.setTodayPages);
  const [subscriptionReady, setSubscriptionReady] = useState(false);
  const [subscriptionFailed, setSubscriptionFailed] = useState(false);
  const [subscriptionAttempt, setSubscriptionAttempt] = useState(0);
  const [desktopActionFailed, setDesktopActionFailed] = useState(false);
  const [onboardingOpen, setOnboardingOpen] = useState<boolean | null>(null);

  useEffect(() => {
    let active = true;
    void getSettings().then(
      (settings) => {
        if (active) setOnboardingOpen(settings.onboarding_complete !== "1");
      },
      () => {
        if (active) setOnboardingOpen(true);
      },
    );
    return () => {
      active = false;
    };
  }, []);

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
      unlisteners.push(await onQueueEvent(upsertTask));
      unlisteners.push(await onUsageUpdated(() => void refreshUsage()));
      unlisteners.push(
        await onCaptureDone((taskId) => {
          setSelectedTaskId(taskId);
          setView("viewer");
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
        () => {
          setDesktopActionFailed(false);
          setView("queue");
        },
        () => setDesktopActionFailed(true),
      );
    };

    window.addEventListener("keydown", pasteClipboardImage);
    return () => window.removeEventListener("keydown", pasteClipboardImage);
  }, [service, setView, view]);

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
    ) : view === "history" ? (
      <History />
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
        <main className="view-content">{content}</main>
      </section>
      <Onboarding open={onboardingOpen === true} onClose={() => setOnboardingOpen(false)} />
      {onboardingOpen === false ? <UpdatePrompt /> : null}
    </div>
  );
}

export default App;
