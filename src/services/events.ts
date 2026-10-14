import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { PlaybackStatus } from "../types";

export const onPlaybackStatus = (cb: (s: PlaybackStatus) => void): Promise<UnlistenFn> =>
  listen<PlaybackStatus>("playback://status", (e) => cb(e.payload));

export const onAlertState = (cb: (active: boolean) => void): Promise<UnlistenFn> =>
  listen<boolean>("alert://state", (e) => cb(e.payload));
