import { describe, expect, it, vi } from "vitest";
import { createShortcutHandler, shortcutRecipient } from "./shortcut";
import type { AlertSendResult, DeviceView, Settings } from "../types";

const settings: Settings = { userName: "Ana", alertVolume: 80, alertDurationSecs: 15, soundEnabled: true, animationsEnabled: true, launchAtLogin: false, shortcutEnabled: true, shortcutDeviceId: null };
const device = (id: string, connected = true): DeviceView => ({ deviceId: id, name: id, os: "windows", connection: connected ? "connected" : "disconnected", pairing: "paired", discovered: true, addresses: [], port: 38473, lastSeenMs: null, lastError: null });
const key = (patch: Record<string, unknown> = {}) => ({ key: "F12", repeat: false, ctrlKey: false, altKey: false, shiftKey: false, metaKey: false, isComposing: false, defaultPrevented: false, preventDefault: vi.fn(), ...patch });
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
    for (const patch of [{ repeat: true }, { ctrlKey: true }, { altKey: true }, { shiftKey: true }, { metaKey: true }, { key: "F8" }, { isComposing: true }]) await handle(key(patch));
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
