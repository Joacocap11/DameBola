import { describe, expect, it, vi } from "vitest";
import {
  beginShortcutCapture, createShortcutCapture, createShortcutHandler, DEFAULT_SHORTCUT, describeShortcut,
  isShortcutCaptureActive, normalizeShortcut, shortcutFromEvent, shortcutRecipient,
} from "./shortcut";
import type { AlertSendResult, DeviceView, Settings } from "../types";

const settings: Settings = { userName: "Ana", alertVolume: 80, alertDurationSecs: 15, soundEnabled: true, animationsEnabled: true, launchAtLogin: false, shortcutEnabled: true, shortcutKey: "F12", shortcutDeviceId: null };
const device = (id: string, connected = true): DeviceView => ({ deviceId: id, name: id, announcedName: id, alias: null, autoConnect: true, os: "windows", connection: connected ? "connected" : "disconnected", pairing: "paired", discovered: true, addresses: [], port: 38473, lastSeenMs: null, lastError: null });
const key = (patch: Record<string, unknown> = {}) => ({ code: "F12", repeat: false, ctrlKey: false, altKey: false, shiftKey: false, metaKey: false, isComposing: false, defaultPrevented: false, preventDefault: vi.fn(), ...patch });
const shown: AlertSendResult = { status: "shown", accepted: true, reason: null, message: "Mostrada" };

describe("F12 recipient", () => {
  it("uses only paired identities, never a newly discovered device", () => {
    const discovered = { ...device("intruder"), pairing: "unpaired" as const };
    expect(shortcutRecipient([discovered, device("mate")], null).deviceId).toBe("mate");
    expect(() => shortcutRecipient([discovered], null)).toThrow("Emparejá");
  });
  it("requires an explicit choice with several pairs, even if only one is online", () => {
    expect(() => shortcutRecipient([device("a"), device("b", false)], null)).toThrow("varios");
    expect(shortcutRecipient([device("a"), device("b")], "b").deviceId).toBe("b");
  });
  it("never falls back to another recipient after the selected pair is forgotten", () => {
    expect(() => shortcutRecipient([device("a")], "b")).toThrow("ya no está emparejado");
  });
});

describe("focused F12 sender", () => {
  it("sends to the paired recipient through the remote-alert API", async () => {
    const send = vi.fn().mockResolvedValue(shown);
    const report = vi.fn();
    const handle = createShortcutHandler({ isFocused: () => true, getState: () => ({ settings, devices: [device("mate")] }), send, report });
    const event = key();
    await handle(event);
    expect(event.preventDefault).toHaveBeenCalledOnce();
    expect(send).toHaveBeenCalledExactlyOnceWith("mate");
    expect(report).toHaveBeenCalledWith("success", "Mostrada");
  });
  it("ignores background input, disabled shortcuts, repeats, modifiers and other keys", async () => {
    const send = vi.fn();
    let focused = false;
    let current = settings;
    const handle = createShortcutHandler({ isFocused: () => focused, getState: () => ({ settings: current, devices: [device("mate")] }), send, report: vi.fn() });
    await handle(key());
    focused = true;
    current = { ...settings, shortcutEnabled: false };
    await handle(key());
    current = settings;
    for (const patch of [{ repeat: true }, { ctrlKey: true }, { altKey: true }, { shiftKey: true }, { metaKey: true }, { code: "F8" }, { isComposing: true }]) await handle(key(patch));
    expect(send).not.toHaveBeenCalled();
  });
  it("does not queue another request while waiting for confirmation", async () => {
    let resolve!: (result: AlertSendResult) => void;
    const send = vi.fn().mockImplementation(() => new Promise<AlertSendResult>((r) => { resolve = r; }));
    const handle = createShortcutHandler({ isFocused: () => true, getState: () => ({ settings, devices: [device("mate")] }), send, report: vi.fn() });
    const first = handle(key());
    await handle(key());
    expect(send).toHaveBeenCalledOnce();
    resolve(shown);
    await first;
    const second = handle(key());
    expect(send).toHaveBeenCalledTimes(2);
    resolve(shown);
    await second;
  });
  it("reports offline without sending or choosing another device", async () => {
    const send = vi.fn();
    const report = vi.fn();
    const handle = createShortcutHandler({ isFocused: () => true, getState: () => ({ settings: { ...settings, shortcutDeviceId: "mate" }, devices: [device("mate", false), device("other")] }), send, report });
    await handle(key());
    expect(send).not.toHaveBeenCalled();
    expect(report).toHaveBeenCalledWith("error", expect.stringContaining("no está conectado"));
  });
  it("does not present an accepted but unconfirmed alert as delivered", async () => {
    const report = vi.fn();
    const handle = createShortcutHandler({ isFocused: () => true, getState: () => ({ settings, devices: [device("mate")] }), send: vi.fn().mockResolvedValue({ ...shown, status: "timeout", message: "Sin confirmación" }), report });
    await handle(key());
    expect(report).toHaveBeenCalledWith("error", "Sin confirmación");
  });
});

// ---------------------------------------------------------------------------
// Atajo configurable
// ---------------------------------------------------------------------------

describe("normalizeShortcut (mismos vectores que src-tauri/src/shortcut.rs)", () => {
  const accepted: [string, string][] = [
    ["F12", "F12"], ["f8", "F8"], ["F24", "F24"], ["Shift+F5", "Shift+F5"], ["ctrl+shift+b", "Ctrl+Shift+B"],
    ["Shift+Ctrl+B", "Ctrl+Shift+B"], ["Cmd+K", "Meta+K"], ["Control+7", "Ctrl+7"], ["Alt+F9", "Alt+F9"],
    ["Ctrl + Shift + B", "Ctrl+Shift+B"], ["Meta+Alt+J", "Alt+Meta+J"],
  ];
  it.each(accepted)("acepta y normaliza %s", (input, expected) => {
    expect(normalizeShortcut(input)).toEqual({ ok: true, value: expected });
  });

  const rejected = [
    "", "+", "F25", "F0", "F01", "B", "7", "Shift+B", "Alt+B", "Space", "Ctrl+Space", "Ctrl+Enter", "Ctrl+Backspace",
    "Ctrl+Delete", "Ctrl+Tab", "Ctrl+ArrowUp", "Escape", "Ctrl+Escape", "Ctrl+Alt+B", "Ctrl+Ctrl+B", "Ctrl+",
    "Ctrl+Shift", "Hyper+B", "Ctrl+C", "Ctrl+V", "Ctrl+X", "Ctrl+A", "Ctrl+Z", "Ctrl+Shift+Z", "Meta+C", "Meta+Q",
    "Ctrl+W", "Ctrl+R", "Alt+F4", "Ctrl+ñ", "B+Ctrl",
  ];
  it.each(rejected)("rechaza «%s»", (input) => {
    expect(normalizeShortcut(input).ok).toBe(false);
  });

  it("F12 sigue siendo el valor predeterminado y es válido", () => {
    expect(DEFAULT_SHORTCUT).toBe("F12");
    expect(normalizeShortcut(DEFAULT_SHORTCUT)).toEqual({ ok: true, value: "F12" });
  });

  it("describe el atajo de forma legible", () => {
    expect(describeShortcut("F8", "other")).toBe("F8");
    expect(describeShortcut("Ctrl+Shift+B", "other")).toBe("Ctrl + Shift + B");
    expect(describeShortcut("Meta+K", "mac")).toBe("Cmd + K");
    expect(describeShortcut("Meta+K", "other")).toBe("Win + K");
  });
});

describe("shortcutFromEvent", () => {
  const ev = (code: string, mods: Partial<Record<"ctrlKey" | "altKey" | "shiftKey" | "metaKey", boolean>> = {}) =>
    ({ code, ctrlKey: false, altKey: false, shiftKey: false, metaKey: false, ...mods });
  it("usa la tecla física: Option+B en macOS (que escribe «∫») sigue siendo B", () => {
    expect(shortcutFromEvent(ev("KeyB", { ctrlKey: true, shiftKey: true }))).toEqual({ kind: "ok", value: "Ctrl+Shift+B" });
    expect(shortcutFromEvent(ev("Digit3", { metaKey: true }))).toEqual({ kind: "ok", value: "Meta+3" });
    expect(shortcutFromEvent(ev("F8"))).toEqual({ kind: "ok", value: "F8" });
  });
  it("trata los modificadores solos como tales y las teclas de edición como no soportadas", () => {
    for (const code of ["ControlLeft", "ShiftRight", "AltLeft", "MetaLeft"]) expect(shortcutFromEvent(ev(code, { ctrlKey: true })).kind).toBe("modifier");
    for (const code of ["Backspace", "Delete", "Enter", "Space", "Tab", "ArrowLeft", "Numpad5", "Escape"]) expect(shortcutFromEvent(ev(code, { ctrlKey: true })).kind).toBe("unsupported");
  });
  it("marca como inválidas las letras sueltas y Ctrl+Alt (AltGr)", () => {
    expect(shortcutFromEvent(ev("KeyB")).kind).toBe("invalid");
    expect(shortcutFromEvent(ev("KeyB", { shiftKey: true })).kind).toBe("invalid");
    expect(shortcutFromEvent(ev("KeyQ", { ctrlKey: true, altKey: true })).kind).toBe("invalid");
  });
});

describe("modo de captura", () => {
  const setup = () => {
    const cb = { onPreview: vi.fn(), onCandidate: vi.fn(), onInvalid: vi.fn(), onCancel: vi.fn() };
    return { cb, handle: createShortcutCapture(cb) };
  };
  const press = (code: string, patch: Record<string, unknown> = {}) => ({ ...key({ code }), ...patch });

  it("captura teclas de función y combinaciones con modificadores", () => {
    const { cb, handle } = setup();
    expect(handle(press("F8"))).toBe(true);
    expect(cb.onCandidate).toHaveBeenLastCalledWith("F8");
    handle(press("KeyB", { ctrlKey: true, shiftKey: true }));
    expect(cb.onCandidate).toHaveBeenLastCalledWith("Ctrl+Shift+B");
  });
  it("Escape cancela; con modificador no cancela", () => {
    const { cb, handle } = setup();
    handle(press("Escape"));
    expect(cb.onCancel).toHaveBeenCalledOnce();
    handle(press("Escape", { shiftKey: true }));
    expect(cb.onCancel).toHaveBeenCalledOnce();
    expect(cb.onCandidate).not.toHaveBeenCalled();
  });
  it("ignora repetición, composición de texto y modificadores sin otra tecla", () => {
    const { cb, handle } = setup();
    handle(press("F8", { repeat: true }));
    handle(press("F8", { isComposing: true }));
    handle(press("KeyB", { keyCode: 229, ctrlKey: true }));
    expect(cb.onCandidate).not.toHaveBeenCalled();
    expect(cb.onInvalid).not.toHaveBeenCalled();
    handle(press("ControlLeft", { ctrlKey: true }));
    expect(cb.onPreview).toHaveBeenLastCalledWith(["Ctrl"]);
    expect(cb.onCandidate).not.toHaveBeenCalled();
  });
  it("explica por qué rechaza letras sueltas, teclas de edición y combinaciones reservadas", () => {
    const { cb, handle } = setup();
    handle(press("KeyB"));
    handle(press("Backspace"));
    handle(press("KeyC", { ctrlKey: true }));
    handle(press("F4", { altKey: true }));
    expect(cb.onInvalid).toHaveBeenCalledTimes(4);
    expect(cb.onCandidate).not.toHaveBeenCalled();
  });
  it("consume todo evento: nunca deja pasar la tecla", () => {
    const { handle } = setup();
    for (const code of ["F12", "KeyB", "Escape", "ControlLeft", "Backspace"]) expect(handle(press(code))).toBe(true);
  });
});

describe("atajo configurable al enviar", () => {
  const mk = (current: Settings, isCapturing?: () => boolean) => {
    const send = vi.fn().mockResolvedValue(shown);
    const report = vi.fn();
    const handle = createShortcutHandler({ isFocused: () => true, isCapturing, getState: () => ({ settings: current, devices: [device("mate")] }), send, report });
    return { send, report, handle };
  };

  it("usa la combinación configurada y deja de responder a F12", async () => {
    const { send, handle } = mk({ ...settings, shortcutKey: "Ctrl+Shift+B" });
    await handle(key({ code: "F12" }));
    await handle(key({ code: "KeyB", ctrlKey: true }));
    await handle(key({ code: "KeyB", shiftKey: true }));
    expect(send).not.toHaveBeenCalled();
    await handle(key({ code: "KeyB", ctrlKey: true, shiftKey: true }));
    expect(send).toHaveBeenCalledExactlyOnceWith("mate");
  });

  it("al cambiar de F12 a F8 la tecla anterior deja de enviar (estado leído en cada pulsación)", async () => {
    let current = settings;
    const send = vi.fn().mockResolvedValue(shown);
    const handle = createShortcutHandler({ isFocused: () => true, getState: () => ({ settings: current, devices: [device("mate")] }), send, report: vi.fn() });
    await handle(key({ code: "F12" }));
    expect(send).toHaveBeenCalledTimes(1);
    current = { ...settings, shortcutKey: "F8" };
    await handle(key({ code: "F12" }));
    expect(send).toHaveBeenCalledTimes(1);
    await handle(key({ code: "F8" }));
    expect(send).toHaveBeenCalledTimes(2);
  });

  it("capturar una combinación NO envía ninguna alerta, ni siquiera la configurada", async () => {
    const { send, handle } = mk(settings, isShortcutCaptureActive);
    const end = beginShortcutCapture();
    await handle(key({ code: "F12" }));
    end();
    expect(send).not.toHaveBeenCalled();
    await handle(key({ code: "F12" }));
    expect(send).toHaveBeenCalledOnce();
  });

  it("anidar capturas no reactiva el envío antes de tiempo y terminar dos veces es inocuo", () => {
    const a = beginShortcutCapture();
    const b = beginShortcutCapture();
    a();
    a();
    expect(isShortcutCaptureActive()).toBe(true);
    b();
    expect(isShortcutCaptureActive()).toBe(false);
  });

  it("cambiar de destinatario o de estado de conexión no permite envíos simultáneos ni selecciones inesperadas", async () => {
    let resolve!: (r: AlertSendResult) => void;
    const send = vi.fn().mockImplementation(() => new Promise<AlertSendResult>((r) => { resolve = r; }));
    let state = { settings: { ...settings, shortcutDeviceId: "a" }, devices: [device("a"), device("b")] };
    const handle = createShortcutHandler({ isFocused: () => true, getState: () => state, send, report: vi.fn() });
    const first = handle(key());
    state = { settings: { ...settings, shortcutDeviceId: "b" }, devices: [device("a"), device("b")] };
    await handle(key());
    expect(send).toHaveBeenCalledExactlyOnceWith("a");
    resolve(shown);
    await first;
    // El destinatario seleccionado se desconecta: no se cambia a otro.
    state = { settings: { ...settings, shortcutDeviceId: "b" }, devices: [device("a"), device("b", false)] };
    await handle(key());
    expect(send).toHaveBeenCalledTimes(1);
  });

  it("los mensajes de error nombran el atajo configurado", async () => {
    const report = vi.fn();
    const handle = createShortcutHandler({ isFocused: () => true, getState: () => ({ settings: { ...settings, shortcutKey: "Ctrl+Shift+B" }, devices: [] }), send: vi.fn(), report });
    await handle(key({ code: "KeyB", ctrlKey: true, shiftKey: true }));
    expect(report).toHaveBeenCalledWith("error", expect.stringContaining("Ctrl + Shift + B"));
  });
});
