import type { AlertSendResult, DeviceView, Settings } from "../types";

// ---------------------------------------------------------------------------
// Formato canónico del atajo: "F8", "Ctrl+Shift+B". Orden de modificadores
// fijo: Ctrl, Alt, Shift, Meta. Las mismas reglas viven en src-tauri/src/shortcut.rs
// (el backend valida y normaliza siempre; esto solo da respuesta inmediata).
// ---------------------------------------------------------------------------

export const DEFAULT_SHORTCUT = "F12";

const MODIFIERS = ["Ctrl", "Alt", "Shift", "Meta"] as const;
type Modifier = (typeof MODIFIERS)[number];

const ALIASES: Record<string, Modifier> = {
  ctrl: "Ctrl", control: "Ctrl", alt: "Alt", option: "Alt", shift: "Shift",
  meta: "Meta", cmd: "Meta", command: "Meta", win: "Meta", super: "Meta",
};

/** Combinaciones con Ctrl/Cmd que ya significan otra cosa (editar, cerrar, recargar). */
const RESERVED_KEYS = new Set(["A", "C", "V", "X", "Z", "Y", "W", "Q", "R", "N", "T"]);

export type ShortcutResult = { ok: true; value: string } | { ok: false; reason: string };

export function normalizeShortcut(raw: string): ShortcutResult {
  const tokens = raw.split("+").map((t) => t.trim());
  if (tokens.some((t) => t === "")) return { ok: false, reason: "Formato de atajo inválido." };
  const keyToken = tokens[tokens.length - 1]!;
  const mods = new Set<Modifier>();
  for (const token of tokens.slice(0, -1)) {
    const mod = ALIASES[token.toLowerCase()];
    if (!mod) return { ok: false, reason: `«${token}» no es un modificador válido.` };
    if (mods.has(mod)) return { ok: false, reason: "Un modificador está repetido." };
    mods.add(mod);
  }
  const fn = /^[Ff]([1-9]|1\d|2[0-4])$/.exec(keyToken);
  let key: string;
  if (fn) key = `F${fn[1]}`;
  else if (/^[A-Za-z0-9]$/.test(keyToken)) key = keyToken.toUpperCase();
  else return { ok: false, reason: "Usá una tecla de función (F1–F24), una letra o un número." };

  const isFunction = fn !== null;
  if (isFunction) {
    if (key === "F4" && mods.has("Alt")) return { ok: false, reason: "Alt + F4 cierra la ventana." };
  } else {
    if (!mods.has("Ctrl") && !mods.has("Meta")) {
      return { ok: false, reason: "Las letras y números necesitan Ctrl (o Cmd) para no enviar alertas al escribir." };
    }
    if (mods.has("Ctrl") && mods.has("Alt")) {
      return { ok: false, reason: "Ctrl + Alt equivale a AltGr en muchos teclados y se usa para escribir." };
    }
    const onlyShiftExtra = [...mods].every((m) => m === "Ctrl" || m === "Meta" || m === "Shift");
    if (RESERVED_KEYS.has(key) && onlyShiftExtra) {
      return { ok: false, reason: `Ctrl/Cmd + ${key} ya tiene otro uso (copiar, pegar, cerrar, recargar…). Elegí otra combinación.` };
    }
  }
  return { ok: true, value: [...MODIFIERS.filter((m) => mods.has(m)), key].join("+") };
}

export type Platform = "mac" | "other";

export function detectPlatform(): Platform {
  return typeof navigator !== "undefined" && /mac/i.test(navigator.platform ?? "") ? "mac" : "other";
}

/** "Ctrl+Shift+B" → "Ctrl + Shift + B" (Meta se muestra como Cmd en macOS y Win en el resto). */
export function describeShortcut(value: string, platform: Platform = detectPlatform()): string {
  return value
    .split("+")
    .map((part) => (part === "Meta" ? (platform === "mac" ? "Cmd" : "Win") : part))
    .join(" + ");
}

export type KeyInput = Pick<KeyboardEvent, "code" | "repeat" | "ctrlKey" | "altKey" | "shiftKey" | "metaKey" | "isComposing" | "defaultPrevented" | "preventDefault"> & { keyCode?: number };

const MODIFIER_CODES = new Set(["ControlLeft", "ControlRight", "AltLeft", "AltRight", "ShiftLeft", "ShiftRight", "MetaLeft", "MetaRight", "OSLeft", "OSRight"]);

export type EventShortcut =
  | { kind: "modifier" }
  | { kind: "unsupported" }
  | { kind: "invalid"; reason: string }
  | { kind: "ok"; value: string };

/** Traduce un keydown a un atajo canónico. Usa `code` (física) para que Alt/Option no cambie la letra. */
export function shortcutFromEvent(e: Pick<KeyInput, "code" | "ctrlKey" | "altKey" | "shiftKey" | "metaKey">): EventShortcut {
  if (MODIFIER_CODES.has(e.code)) return { kind: "modifier" };
  const key = /^F(\d{1,2})$/.exec(e.code)?.[0] ?? /^Key([A-Z])$/.exec(e.code)?.[1] ?? /^Digit(\d)$/.exec(e.code)?.[1];
  if (!key) return { kind: "unsupported" };
  const mods = [e.ctrlKey && "Ctrl", e.altKey && "Alt", e.shiftKey && "Shift", e.metaKey && "Meta"].filter(Boolean) as string[];
  const result = normalizeShortcut([...mods, key].join("+"));
  return result.ok ? { kind: "ok", value: result.value } : { kind: "invalid", reason: result.reason };
}

// ---------------------------------------------------------------------------
// Captura: modo "presioná la combinación". Nunca envía alertas.
// ---------------------------------------------------------------------------

let captureDepth = 0;

/** Mientras sea true, el atajo de envío está suspendido. */
export function isShortcutCaptureActive(): boolean {
  return captureDepth > 0;
}

export function beginShortcutCapture(): () => void {
  captureDepth += 1;
  let ended = false;
  return () => {
    if (!ended) {
      ended = true;
      captureDepth -= 1;
    }
  };
}

export interface CaptureCallbacks {
  onPreview: (modifiers: string[]) => void;
  onCandidate: (value: string) => void;
  onInvalid: (reason: string) => void;
  onCancel: () => void;
}

/**
 * Devuelve el manejador de keydown del modo captura. Siempre "consume" el evento
 * (retorna true): el llamador debe hacer preventDefault + stopPropagation.
 */
export function createShortcutCapture(cb: CaptureCallbacks) {
  return (event: KeyInput): true => {
    if (event.isComposing || event.keyCode === 229) return true; // composición de texto (IME)
    if (event.repeat) return true;
    const plain = !event.ctrlKey && !event.altKey && !event.shiftKey && !event.metaKey;
    if (event.code === "Escape" && plain) {
      cb.onCancel();
      return true;
    }
    const parsed = shortcutFromEvent(event);
    switch (parsed.kind) {
      case "modifier":
        cb.onPreview([event.ctrlKey && "Ctrl", event.altKey && "Alt", event.shiftKey && "Shift", event.metaKey && "Meta"].filter(Boolean) as string[]);
        break;
      case "unsupported":
        cb.onInvalid("Esa tecla no se puede usar. Probá con F1–F24, o Ctrl/Cmd + letra o número.");
        break;
      case "invalid":
        cb.onInvalid(parsed.reason);
        break;
      case "ok":
        cb.onCandidate(parsed.value);
        break;
    }
    return true;
  };
}

// ---------------------------------------------------------------------------
// Envío
// ---------------------------------------------------------------------------

export function shortcutRecipient(devices: DeviceView[], targetId: string | null, label = "el atajo"): DeviceView {
  const paired = devices.filter((d) => d.pairing === "paired");
  if (targetId !== null) {
    const target = paired.find((d) => d.deviceId === targetId);
    if (!target) throw new Error(`El destinatario de ${label} ya no está emparejado. Elegí otro en Configuración.`);
    return target;
  }
  if (paired.length === 0) throw new Error(`Emparejá un dispositivo en Dispositivos antes de usar ${label}.`);
  if (paired.length !== 1) throw new Error(`Elegí el destinatario de ${label} en Configuración: hay varios equipos emparejados.`);
  return paired[0]!;
}

type Report = (kind: "success" | "warning" | "error", message: string) => void;

/** Main-window handler. One request at a time; holding the key never queues alerts. */
export function createShortcutHandler(options: {
  isFocused: () => boolean;
  isCapturing?: () => boolean;
  getState: () => { settings: Settings | null; devices: DeviceView[] };
  send: (deviceId: string) => Promise<AlertSendResult>;
  report: Report;
}) {
  let pending = false;
  return async (event: KeyInput): Promise<void> => {
    const { settings, devices } = options.getState();
    if (!settings?.shortcutEnabled || !options.isFocused() || options.isCapturing?.() || event.isComposing || event.defaultPrevented) return;
    const input = shortcutFromEvent(event);
    if (input.kind !== "ok" || input.value !== settings.shortcutKey) return;
    event.preventDefault();
    if (event.repeat || pending) return;
    pending = true;
    const label = describeShortcut(settings.shortcutKey);
    try {
      const recipient = shortcutRecipient(devices, settings.shortcutDeviceId, label);
      if (recipient.connection !== "connected") throw new Error(`${recipient.name} no está conectado. No se guardó ninguna alerta.`);
      const result = await options.send(recipient.deviceId);
      options.report(result.status === "shown" ? "success" : result.status === "rejected" ? "warning" : "error", result.message);
    } catch (error) {
      options.report("error", error instanceof Error ? error.message : typeof error === "string" ? error : "No se pudo enviar DAME BOLA.");
    } finally {
      pending = false;
    }
  };
}
