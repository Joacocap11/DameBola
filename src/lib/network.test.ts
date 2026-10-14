import { describe, expect, it } from "vitest";
import {
  formatRelativeTime,
  isMuted,
  muteUntil,
  validateCooldown,
  validateHost,
  validatePort,
} from "./network";

describe("validateHost", () => {
  it("acepta IPv4 válidas", () => {
    for (const ip of ["192.168.1.20", "0.0.0.0", "255.255.255.255", " 10.0.0.1 "]) expect(validateHost(ip)).toBeNull();
  });
  it("rechaza IPv4 inválidas, vacío y hostnames", () => {
    for (const bad of ["", "   ", "256.1.1.1", "1.2.3", "1.2.3.4.5", "1.2.3.a", "01234.1.1.1", "mi-pc.local", "localhost", "http://1.2.3.4"]) {
      expect(validateHost(bad)).not.toBeNull();
    }
  });
  it("acepta IPv6 básicas", () => {
    for (const ip of ["::1", "fe80::1", "2001:db8::ff00:42:8329", "2001:0db8:0000:0000:0000:ff00:0042:8329", "::ffff:192.168.1.1", "::"]) {
      expect(validateHost(ip)).toBeNull();
    }
  });
  it("rechaza IPv6 inválidas", () => {
    for (const bad of ["1::2::3", ":::", "12345::1", "1:2:3:4:5:6:7", "g::1", "1:2:3:4:5:6:7:8:9", "::1.2.3.999"]) {
      expect(validateHost(bad)).not.toBeNull();
    }
  });
});

describe("validatePort", () => {
  it("acepta el rango 1024..65535", () => {
    for (const p of [1024, 38473, 65535, "5000"]) expect(validatePort(p)).toBeNull();
  });
  it("rechaza fuera de rango y no enteros", () => {
    for (const p of [1023, 65536, 0, -1, "", "abc", "12.5", "1e3"]) expect(validatePort(p)).not.toBeNull();
  });
});

describe("validateCooldown", () => {
  it("valida 5..3600", () => {
    expect(validateCooldown(5)).toBeNull();
    expect(validateCooldown("3600")).toBeNull();
    expect(validateCooldown(4)).not.toBeNull();
    expect(validateCooldown(3601)).not.toBeNull();
    expect(validateCooldown("x")).not.toBeNull();
  });
});

describe("formatRelativeTime", () => {
  const now = 1_000_000_000;
  it("formatea segundos, minutos, horas y días", () => {
    expect(formatRelativeTime(now, now)).toBe("ahora mismo");
    expect(formatRelativeTime(now + 500, now)).toBe("ahora mismo");
    expect(formatRelativeTime(now - 5_000, now)).toBe("hace 5 s");
    expect(formatRelativeTime(now - 59_000, now)).toBe("hace 59 s");
    expect(formatRelativeTime(now - 180_000, now)).toBe("hace 3 min");
    expect(formatRelativeTime(now - 2 * 3_600_000, now)).toBe("hace 2 h");
    expect(formatRelativeTime(now - 3 * 86_400_000, now)).toBe("hace 3 d");
  });
});

describe("muteUntil", () => {
  const now = new Date(2026, 5, 10, 15, 30).getTime();
  it("suma 15 min y 1 h", () => {
    expect(muteUntil("15m", now)).toBe(now + 900_000);
    expect(muteUntil("1h", now)).toBe(now + 3_600_000);
  });
  it("mañana = 08:00 del día siguiente", () => {
    expect(muteUntil("tomorrow", now)).toBe(new Date(2026, 5, 11, 8, 0).getTime());
  });
  it("isMuted respeta null y el pasado", () => {
    expect(isMuted(null, now)).toBe(false);
    expect(isMuted(now - 1, now)).toBe(false);
    expect(isMuted(now + 1, now)).toBe(true);
  });
});
