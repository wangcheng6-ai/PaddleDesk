import { useEffect } from "react";
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

  useEffect(() => {
    let disposed = false;
    let cleanup: (() => void) | undefined;

    void onQueueEvent(upsertTask).then(
      (unlisten) => {
        if (disposed) unlisten();
        else cleanup = unlisten;
      },
      (error) => {
        if (!disposed) setTimeout(() => { throw error; });
      },
    );

    return () => {
      disposed = true;
      cleanup?.();
    };
  }, [upsertTask]);

  return (
    <div className="app-shell">
      <Sidebar />
      <section className="workspace">
        <TopBar />
        <main className="view-content">
          <h1>{t(titleKeys[view])}</h1>
        </main>
      </section>
    </div>
  );
}

export default App;
