import type { Song } from "../types";

export type StatusFilter = "all" | "active" | "inactive";
export type SortKey = "name" | "date";
export type SortDirection = "asc" | "desc";

export interface SongView {
  query: string;
  status: StatusFilter;
  sortKey: SortKey;
  direction: SortDirection;
}

export const DEFAULT_VIEW: SongView = { query: "", status: "all", sortKey: "date", direction: "desc" };

/** Minúsculas y sin acentos, para búsquedas tolerantes. */
export function normalize(text: string): string {
  return text.normalize("NFD").replace(/\p{Diacritic}/gu, "").toLowerCase().trim();
}

export function filterAndSortSongs(songs: readonly Song[], view: SongView): Song[] {
  const q = normalize(view.query);
  const filtered = songs.filter((s) => {
    if (view.status === "active" && !s.enabled) return false;
    if (view.status === "inactive" && s.enabled) return false;
    return q === "" || normalize(s.name).includes(q);
  });
  const sign = view.direction === "asc" ? 1 : -1;
  return filtered.sort((a, b) => {
    const cmp =
      view.sortKey === "name"
        ? a.name.localeCompare(b.name, "es", { sensitivity: "base", numeric: true })
        : a.createdAt - b.createdAt;
    return cmp !== 0 ? sign * cmp : a.id.localeCompare(b.id);
  });
}

/** Canciones que pueden sonar en una alerta (activas con archivo presente). */
export function countPlayable(songs: readonly Song[]): number {
  return songs.filter((s) => s.enabled && !s.fileMissing).length;
}
