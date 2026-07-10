import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { Sidebar } from "./components/Sidebar";
import { TopBar } from "./components/TopBar";
import { onQueueEvent } from "./lib/ipc";
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
  const [subscriptionReady, setSubscriptionReady] = useState(false);
  const [subscriptionFailed, setSubscriptionFailed] = useState(false);
  const [subscriptionAttempt, setSubscriptionAttempt] = useState(0);

  useEffect(() => {
    let disposed = false;
    let cleanup: (() => void) | undefined;

    void onQueueEvent(upsertTask).then(
      (unlisten) => {
        if (disposed) unlisten();
        else {
          cleanup = unlisten;
          setSubscriptionReady(true);
          setSubscriptionFailed(false);
        }
      },
      () => {
        if (!disposed) {
          setSubscriptionReady(false);
          setSubscriptionFailed(true);
        }
      },
    );

    return () => {
      disposed = true;
      cleanup?.();
    };
  }, [subscriptionAttempt, upsertTask]);

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
