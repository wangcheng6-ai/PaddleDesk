import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { Sidebar } from "./components/Sidebar";
import { TopBar } from "./components/TopBar";
import { getUsage, onQueueEvent, onUsageUpdated } from "./lib/ipc";
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
  const upsertTask = useApp((state) => state.upsertTask);
  const setTodayPages = useApp((state) => state.setTodayPages);
  const [subscriptionReady, setSubscriptionReady] = useState(false);
  const [subscriptionFailed, setSubscriptionFailed] = useState(false);
  const [subscriptionAttempt, setSubscriptionAttempt] = useState(0);

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
  }, [setTodayPages, subscriptionAttempt, upsertTask]);

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
      <Settings />
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
        <main className="view-content">{content}</main>
      </section>
    </div>
  );
}

export default App;
