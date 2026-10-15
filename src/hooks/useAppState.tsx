import { createContext, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from "react";
import { api, errorMessage } from "../services/api";
import { onAlertState, onPlaybackStatus, onSettingsChanged } from "../services/events";
import type { AppInfo, PlaybackStatus, Settings, Song } from "../types";
import { useToasts } from "../components/Toasts";

interface AppState {
  info: AppInfo | null;
  /** `null` mientras carga por primera vez. */
  songs: Song[] | null;
  songsError: string | null;
  refreshSongs: () => Promise<void>;
  settings: Settings | null;
  saveSettings: (s: Settings) => Promise<boolean>;
  playback: PlaybackStatus;
  alertActive: boolean;
}

const IDLE: PlaybackStatus = { state: "stopped", songId: null, origin: null };
const Ctx = createContext<AppState | null>(null);

export function useAppState(): AppState {
  const ctx = useContext(Ctx);
  if (!ctx) throw new Error("useAppState debe usarse dentro de <AppStateProvider>");
  return ctx;
}

export function AppStateProvider({ children }: { children: ReactNode }) {
  const { notify } = useToasts();
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [songs, setSongs] = useState<Song[] | null>(null);
  const [songsError, setSongsError] = useState<string | null>(null);
  const [settings, setSettings] = useState<Settings | null>(null);
  const [playback, setPlayback] = useState<PlaybackStatus>(IDLE);
  const [alertActive, setAlertActive] = useState(false);

  const refreshSongs = useCallback(async () => {
    try {
      setSongs(await api.listSongs());
      setSongsError(null);
    } catch (e) {
      setSongsError(errorMessage(e));
    }
  }, []);

  const saveSettings = useCallback(
    async (next: Settings) => {
      try {
        setSettings(await api.saveSettings(next));
        return true;
      } catch (e) {
        notify("error", errorMessage(e));
        return false;
      }
    },
    [notify],
  );

  useEffect(() => {
    void refreshSongs();
    api.getAppInfo().then(setInfo).catch((e) => notify("error", errorMessage(e)));
    api.getSettings().then(setSettings).catch((e) => notify("error", errorMessage(e)));
    api.getPlaybackStatus().then(setPlayback).catch(() => undefined);
    api.isAlertActive().then(setAlertActive).catch(() => undefined);

    const unlisten = [onPlaybackStatus(setPlayback), onAlertState(setAlertActive), onSettingsChanged(setSettings)];
    return () => {
      unlisten.forEach((p) => void p.then((fn) => fn()));
    };
  }, [refreshSongs, notify]);

  const value = useMemo(
    () => ({ info, songs, songsError, refreshSongs, settings, saveSettings, playback, alertActive }),
    [info, songs, songsError, refreshSongs, settings, saveSettings, playback, alertActive],
  );
  return <Ctx.Provider value={value}>{children}</Ctx.Provider>;
}
