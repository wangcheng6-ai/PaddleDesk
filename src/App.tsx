import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { Sidebar } from "./components/Sidebar";
import { TopBar } from "./components/TopBar";
import { onQueueEvent } from "./lib/ipc";
import { useApp, type View } from "./stores/app";

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
          setSubscriptionFailed(false);
        }
      },
      () => {
        if (!disposed) setSubscriptionFailed(true);
      },
    );

    return () => {
      disposed = true;
      cleanup?.();
    };
  }, [subscriptionAttempt, upsertTask]);

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
              onClick={() => setSubscriptionAttempt((attempt) => attempt + 1)}
            >
              {t("actions.retry")}
            </button>
          </div>
        )}
        <main className="view-content">
          <h1>{t(titleKeys[view])}</h1>
        </main>
      </section>
    </div>
  );
}

export default App;
