import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import { useTranslation } from "react-i18next";
import { getCurrentWebview, type DragDropEvent } from "@tauri-apps/api/webview";
import { open } from "@tauri-apps/plugin-dialog";

import { filterSupported } from "../lib/files";

const EXTENSIONS = ["png", "jpg", "jpeg", "webp", "pdf"];

interface DropZoneProps {
  onPaths: (paths: string[]) => void | Promise<void>;
}

export function DropZone({ onPaths }: DropZoneProps) {
  const { t } = useTranslation();
  const [registrationError, setRegistrationError] = useState(false);
  const [submitError, setSubmitError] = useState(false);
  const [registrationAttempt, setRegistrationAttempt] = useState(0);
  const onPathsRef = useRef(onPaths);

  useLayoutEffect(() => {
    onPathsRef.current = onPaths;
  }, [onPaths]);

  const submit = useCallback((paths: string[]) => {
    const supported = filterSupported(paths);
    if (supported.length === 0) return;
    let submission: void | Promise<void>;
    try {
      submission = onPathsRef.current(supported);
    } catch {
      setSubmitError(true);
      return;
    }
    void Promise.resolve(submission).then(
      () => setSubmitError(false),
      () => setSubmitError(true),
    );
  }, []);

  useEffect(() => {
    let disposed = false;
    let cleanup: (() => void) | undefined;
    const handleDragDrop = ({ payload }: { payload: DragDropEvent }) => {
      if (payload.type === "drop") submit(payload.paths);
    };

    void getCurrentWebview().onDragDropEvent(handleDragDrop).then(
      (unlisten) => {
        if (disposed) unlisten();
        else {
          cleanup = unlisten;
          setRegistrationError(false);
        }
      },
      () => {
        if (!disposed) setRegistrationError(true);
      },
    );

    return () => {
      disposed = true;
      cleanup?.();
    };
  }, [registrationAttempt, submit]);

  const chooseFiles = async () => {
    try {
      const selected = await open({
        multiple: true,
        filters: [{ name: t("home.supportedFiles"), extensions: EXTENSIONS }],
      });
      submit(selected ?? []);
    } catch {
      setSubmitError(true);
    }
  };

  return (
    <section className="drop-zone" aria-labelledby="drop-zone-title">
      <span className="drop-zone-mark" aria-hidden="true" />
      <h2 id="drop-zone-title">{t("home.dropTitle")}</h2>
      <p>{t("home.dropDescription")}</p>
      <p className="cloud-disclosure">{t("home.cloudDisclosure")}</p>
      <button className="secondary-button" type="button" onClick={chooseFiles}>
        {t("actions.chooseFiles")}
      </button>
      {registrationError && (
        <div className="drop-zone-alert" role="alert">
          <span>{t("home.dropRegistrationFailed")}</span>
          <button
            type="button"
            onClick={() => setRegistrationAttempt((attempt) => attempt + 1)}
          >
            {t("actions.retry")}
          </button>
        </div>
      )}
      {submitError && <p role="alert">{t("home.submitFailed")}</p>}
    </section>
  );
}
