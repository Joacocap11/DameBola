import { describe, expect, it } from "vitest";
import { countPlayable, DEFAULT_VIEW, filterAndSortSongs, type SongView } from "./songs";
import { formatBytes } from "./format";
import type { Song } from "../types";

const song = (over: Partial<Song> & Pick<Song, "id" | "name">): Song => ({
  originalFilename: `${over.name}.mp3`,
  format: "mp3",
  sizeBytes: 1000,
  sha256: "0".repeat(64),
  enabled: true,
  createdAt: 0,
  updatedAt: 0,
  fileMissing: false,
  ...over,
});

const songs = [
  song({ id: "1", name: "Zamba del pollo", createdAt: 300 }),
  song({ id: "2", name: "álamo", createdAt: 100, enabled: false }),
  song({ id: "3", name: "Bailando", createdAt: 200 }),
];
const view = (v: Partial<SongView>): SongView => ({ ...DEFAULT_VIEW, ...v });
const ids = (list: Song[]) => list.map((s) => s.id);

describe("filterAndSortSongs", () => {
  it("ordena por fecha descendente por defecto", () => {
    expect(ids(filterAndSortSongs(songs, DEFAULT_VIEW))).toEqual(["1", "3", "2"]);
  });

  it("ordena por nombre ignorando acentos y mayúsculas", () => {
    expect(ids(filterAndSortSongs(songs, view({ sortKey: "name", direction: "asc" })))).toEqual(["2", "3", "1"]);
    expect(ids(filterAndSortSongs(songs, view({ sortKey: "name", direction: "desc" })))).toEqual(["1", "3", "2"]);
  });

  it("busca sin distinguir acentos", () => {
    expect(ids(filterAndSortSongs(songs, view({ query: "ALAMO" })))).toEqual(["2"]);
    expect(ids(filterAndSortSongs(songs, view({ query: "  pollo " })))).toEqual(["1"]);
    expect(filterAndSortSongs(songs, view({ query: "inexistente" }))).toEqual([]);
  });

  it("filtra por estado y combina con la búsqueda", () => {
    expect(ids(filterAndSortSongs(songs, view({ status: "inactive" })))).toEqual(["2"]);
    expect(ids(filterAndSortSongs(songs, view({ status: "active", query: "bail" })))).toEqual(["3"]);
  });

  it("no modifica el arreglo original", () => {
    const copy = [...songs];
    filterAndSortSongs(songs, view({ sortKey: "name" }));
    expect(songs).toEqual(copy);
  });
});

describe("countPlayable", () => {
  it("excluye inactivas y con archivo faltante", () => {
    const list = [...songs, song({ id: "4", name: "x", fileMissing: true })];
    expect(countPlayable(list)).toBe(2);
  });
});

describe("formatBytes", () => {
  it("formatea unidades", () => {
    expect(formatBytes(512)).toBe("512 B");
    expect(formatBytes(1536)).toBe("1.5 KB");
    expect(formatBytes(5 * 1024 * 1024)).toBe("5.0 MB");
  });
});
