import { useEffect, useRef } from "react";
import { useToasts } from "../components/Toasts";
import { createShortcutHandler, isShortcutCaptureActive } from "../lib/shortcut";
import { api } from "../services/api";
import { onSendResult } from "../services/events";
import { useAppState } from "./useAppState";

export function useDameBolaShortcut() {
  const { settings } = useAppState();
  const { notify } = useToasts();
  const state = useRef({ settings });
  useEffect(() => { state.current = { settings }; }, [settings]);

  useEffect(() => {
    const handle = createShortcutHandler({
      isFocused: () => document.hasFocus(),
      isCapturing: isShortcutCaptureActive,
      getState: () => state.current,
      send: api.sendShortcutAlert,
      report: notify,
    });
    const onKey = (event: KeyboardEvent) => { void handle(event); };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [notify]);

  // Envíos iniciados en el backend (atajo global o bandeja): el resultado se muestra si hay ventana visible.
  useEffect(() => {
    const unlisten = onSendResult((r) => notify(r.status === "shown" ? "success" : r.status === "rejected" ? "warning" : "error", r.message));
    return () => void unlisten.then((fn) => fn());
  }, [notify]);
}
