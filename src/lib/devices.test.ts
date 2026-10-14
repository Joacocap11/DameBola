import { describe, expect, it } from "vitest";
import {
  announcedAsLabel,
  connectionLabel,
  isResetConfirmed,
  MAX_ALIAS_LENGTH,
  RESET_CONFIRMATION,
  validateAlias,
} from "./devices";

describe("connectionLabel", () => {
  it("distingue desconexión voluntaria", () => {
    expect(connectionLabel({ connection: "disconnected", autoConnect: false })).toBe("Desconectado por vos");
    expect(connectionLabel({ connection: "disconnected", autoConnect: true })).toBe("Desconectado");
  });
  it("conectado y conectando no dependen de autoConnect", () => {
    expect(connectionLabel({ connection: "connected", autoConnect: false })).toBe("Conectado");
    expect(connectionLabel({ connection: "connecting", autoConnect: false })).toBe("Conectando…");
  });
});

describe("validateAlias", () => {
  it("acepta vacío, normal y el máximo", () => {
    expect(validateAlias("")).toBeNull();
    expect(validateAlias("  Oficina  ")).toBeNull();
    expect(validateAlias("a".repeat(MAX_ALIAS_LENGTH))).toBeNull();
  });
  it("rechaza largos y caracteres de control", () => {
    expect(validateAlias("a".repeat(MAX_ALIAS_LENGTH + 1))).not.toBeNull();
    expect(validateAlias("a\nb")).not.toBeNull();
  });
});

describe("reset", () => {
  it("exige la frase exacta", () => {
    expect(RESET_CONFIRMATION).toBe("NUEVA IDENTIDAD");
    expect(isResetConfirmed("NUEVA IDENTIDAD")).toBe(true);
    expect(isResetConfirmed("nueva identidad")).toBe(false);
    expect(isResetConfirmed("NUEVA IDENTIDAD ")).toBe(false);
  });
});

describe("announcedAsLabel", () => {
  it("solo con alias distinto", () => {
    expect(announcedAsLabel({ alias: "Mesa", announcedName: "PC-1" })).toBe("Anunciado como PC-1");
    expect(announcedAsLabel({ alias: null, announcedName: "PC-1" })).toBeNull();
    expect(announcedAsLabel({ alias: "PC-1", announcedName: "PC-1" })).toBeNull();
    expect(announcedAsLabel({ alias: "Mesa", announcedName: null })).toBeNull();
  });
});
