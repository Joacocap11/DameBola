import { useEffect, useRef } from "react";
import { useToasts } from "../components/Toasts";
import { createShortcutHandler, isShortcutCaptureActive } from "../lib/shortcut";
import { api } from "../services/api";
import { useAppState } from "./useAppState";
import { useNetworkState } from "./useNetworkState";

export function useDameBolaShortcut() {
  const { settings } = useAppState();
  const { devices } = useNetworkState();
  const { notify } = useToasts();
  const state = useRef({ settings, devices });
  useEffect(() => { state.current = { settings, devices }; }, [settings, devices]);

  useEffect(() => {
    const handle = createShortcutHandler({
      isFocused: () => document.hasFocus(),
      isCapturing: isShortcutCaptureActive,
      getState: () => state.current,
      send: api.netSendAlert,
      report: notify,
    });
    const onKey = (event: KeyboardEvent) => { void handle(event); };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [notify]);
}
