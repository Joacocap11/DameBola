import { useMemo, useState } from "react";
import { ArrowDownAZ, ArrowUpAZ, FileAudio, Loader2, Pause, Pencil, Play, Plus, Search, Square, Trash2, TriangleAlert } from "lucide-react";
import { Button, Card, IconButton, Modal, PageHeader, Switch } from "../components/ui";
import { useToasts } from "../components/Toasts";
import { useAppState } from "../hooks/useAppState";
import { DEFAULT_VIEW, filterAndSortSongs, type SongView, type SortKey, type StatusFilter } from "../lib/songs";
import { formatBytes, formatDate, pluralSongs } from "../lib/format";
import { api, errorMessage } from "../services/api";
import type { ImportReport, Song } from "../types";

function summarizeImport(report: ImportReport): { kind: "success" | "warning" | "error"; message: string }[] {
  const out: { kind: "success" | "warning" | "error"; message: string }[] = [];
  const fmt = (list: { fileName: string; reason: string }[]) => list.map((i) => `• ${i.fileName}: ${i.reason}`).join("\n");
  if (report.imported.length > 0) {
    out.push({ kind: "success", message: `Se agregaron ${pluralSongs(report.imported.length)}.` });
  }
  if (report.duplicates.length > 0) {
    out.push({ kind: "warning", message: `Duplicadas (omitidas):\n${fmt(report.duplicates)}` });
  }
  if (report.failed.length > 0) {
    out.push({ kind: "error", message: `No se pudieron agregar:\n${fmt(report.failed)}` });
  }
  return out;
}

export function SongsPage() {
  const { songs, songsError, refreshSongs, playback } = useAppState();
  const { notify } = useToasts();
  const [view, setView] = useState<SongView>(DEFAULT_VIEW);
  const [importing, setImporting] = useState(false);
  const [editing, setEditing] = useState<Song | null>(null);
  const [deleting, setDeleting] = useState<Song | null>(null);

  const visible = useMemo(() => (songs ? filterAndSortSongs(songs, view) : []), [songs, view]);
  const patchView = (p: Partial<SongView>) => setView((v) => ({ ...v, ...p }));

  async function run<T>(action: () => Promise<T>): Promise<T | undefined> {
    try {
      return await action();
    } catch (e) {
      notify("error", errorMessage(e));
      return undefined;
    }
  }

  async function addSongs() {
    setImporting(true);
    const report = await run(api.importSongs);
    setImporting(false);
    if (!report || report.cancelled) return;
    summarizeImport(report).forEach((m) => notify(m.kind, m.message));
    await refreshSongs();
  }

  async function toggleEnabled(song: Song, enabled: boolean) {
    if (await run(() => api.setSongEnabled(song.id, enabled))) await refreshSongs();
  }

  async function confirmDelete(song: Song) {
    const ok = await run(() => api.deleteSong(song.id).then(() => true));
    setDeleting(null);
    if (ok) {
      notify("success", `«${song.name}» se eliminó.`);
      await refreshSongs();
    }
  }

  const togglePlay = (song: Song) => {
    const mine = playback.songId === song.id;
    if (mine && playback.state === "playing") return run(api.pausePlayback);
    if (mine && playback.state === "paused") return run(api.resumePlayback);
    return run(() => api.playSong(song.id));
  };

  return (
    <div className="mx-auto max-w-6xl animate-fade-in">
      <PageHeader
        title="Canciones"
        subtitle="Biblioteca de canciones que pueden sonar en una alerta."
        actions={
          <Button variant="primary" icon={<Plus className="size-4" />} loading={importing} onClick={addSongs}>
            Agregar canciones
          </Button>
        }
      />

      <div className="mb-4 flex flex-wrap items-center gap-3">
        <div className="relative min-w-60 flex-1">
          <Search className="absolute left-3 top-1/2 size-4 -translate-y-1/2 text-zinc-500" />
          <input
            value={view.query}
            onChange={(e) => patchView({ query: e.target.value })}
            placeholder="Buscar por nombre…"
            className="w-full rounded-xl border border-zinc-800 bg-zinc-900 py-2 pl-9 pr-3 text-sm outline-none transition-colors focus:border-red-500"
          />
        </div>
        <select aria-label="Filtrar por estado" value={view.status} onChange={(e) => patchView({ status: e.target.value as StatusFilter })} className="rounded-xl border border-zinc-800 bg-zinc-900 px-3 py-2 text-sm outline-none focus:border-red-500">
          <option value="all">Todas</option>
          <option value="active">Activas</option>
          <option value="inactive">Inactivas</option>
        </select>
        <select aria-label="Ordenar por" value={view.sortKey} onChange={(e) => patchView({ sortKey: e.target.value as SortKey })} className="rounded-xl border border-zinc-800 bg-zinc-900 px-3 py-2 text-sm outline-none focus:border-red-500">
          <option value="date">Ordenar por fecha</option>
          <option value="name">Ordenar por nombre</option>
        </select>
        <IconButton label={view.direction === "asc" ? "Orden ascendente" : "Orden descendente"} onClick={() => patchView({ direction: view.direction === "asc" ? "desc" : "asc" })} className="border border-zinc-800 bg-zinc-900">
          {view.direction === "asc" ? <ArrowUpAZ className="size-4" /> : <ArrowDownAZ className="size-4" />}
        </IconButton>
        {songs && (
          <span className="text-sm text-zinc-400" aria-live="polite">
            {visible.length === songs.length ? pluralSongs(songs.length) : `${visible.length} de ${pluralSongs(songs.length)}`}
          </span>
        )}
      </div>

      {songsError && (
        <Card className="mb-4 flex items-center gap-3 border-red-900 p-4 text-sm text-red-300">
          <TriangleAlert className="size-5" /> {songsError}
          <Button onClick={() => void refreshSongs()} className="ml-auto">Reintentar</Button>
        </Card>
      )}

      {songs === null && !songsError ? (
        <Card className="flex items-center justify-center gap-3 p-16 text-zinc-400">
          <Loader2 className="size-5 animate-spin" /> Cargando canciones…
        </Card>
      ) : songs && songs.length === 0 ? (
        <Card className="flex flex-col items-center gap-4 p-16 text-center">
          <FileAudio className="size-14 text-zinc-600" />
          <div>
            <p className="text-lg font-bold">Todavía no hay canciones</p>
            <p className="mt-1 text-sm text-zinc-400">Agregá archivos MP3, WAV u OGG. Sin canciones, la alerta se muestra igual pero sin sonido.</p>
          </div>
          <Button variant="primary" icon={<Plus className="size-4" />} loading={importing} onClick={addSongs}>Agregar canciones</Button>
        </Card>
      ) : songs && visible.length === 0 ? (
        <Card className="p-12 text-center text-sm text-zinc-400">Ninguna canción coincide con los filtros.</Card>
      ) : (
        <Card className="overflow-hidden">
          <table className="w-full text-left text-sm">
            <thead className="border-b border-zinc-800 text-xs uppercase tracking-wider text-zinc-500">
              <tr>
                <th className="w-28 px-4 py-3">Reproducir</th>
                <th className="px-4 py-3">Nombre</th>
                <th className="px-4 py-3">Formato</th>
                <th className="px-4 py-3">Tamaño</th>
                <th className="px-4 py-3">Incorporada</th>
                <th className="px-4 py-3">Activa</th>
                <th className="w-24 px-4 py-3 text-right">Acciones</th>
              </tr>
            </thead>
            <tbody>
              {visible.map((song) => {
                const mine = playback.songId === song.id && playback.state !== "stopped";
                const playing = mine && playback.state === "playing";
                return (
                  <tr key={song.id} className={`border-b border-zinc-800/60 last:border-0 transition-colors hover:bg-zinc-800/30 ${mine ? "bg-red-950/20" : ""}`}>
                    <td className="px-4 py-2">
                      <div className="flex gap-1">
                        <IconButton label={playing ? "Pausar" : "Reproducir"} disabled={song.fileMissing} onClick={() => void togglePlay(song)}>
                          {playing ? <Pause className="size-4" /> : <Play className="size-4" />}
                        </IconButton>
                        <IconButton label="Detener" disabled={!mine} onClick={() => void run(api.stopPlayback)}>
                          <Square className="size-4" />
                        </IconButton>
                      </div>
                    </td>
                    <td className="max-w-xs px-4 py-2">
                      <div className="truncate font-medium" title={song.name}>{song.name}</div>
                      {song.fileMissing && <div className="flex items-center gap-1 text-xs text-amber-400"><TriangleAlert className="size-3" /> Archivo faltante</div>}
                    </td>
                    <td className="px-4 py-2"><span className="rounded-md bg-zinc-800 px-2 py-0.5 text-xs font-semibold uppercase">{song.format}</span></td>
                    <td className="px-4 py-2 text-zinc-400">{formatBytes(song.sizeBytes)}</td>
                    <td className="px-4 py-2 text-zinc-400">{formatDate(song.createdAt)}</td>
                    <td className="px-4 py-2">
                      <Switch checked={song.enabled} label={song.enabled ? "Desactivar" : "Activar"} onChange={(v) => void toggleEnabled(song, v)} />
                    </td>
                    <td className="px-4 py-2">
                      <div className="flex justify-end gap-1">
                        <IconButton label="Editar" onClick={() => setEditing(song)}><Pencil className="size-4" /></IconButton>
                        <IconButton label="Eliminar" className="hover:text-red-400" onClick={() => setDeleting(song)}><Trash2 className="size-4" /></IconButton>
                      </div>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </Card>
      )}

      {editing && <EditModal song={editing} onClose={() => setEditing(null)} onSaved={async () => { setEditing(null); await refreshSongs(); }} />}

      {deleting && (
        <Modal title="Eliminar canción" onClose={() => setDeleting(null)}>
          <p className="text-sm text-zinc-300">
            ¿Eliminar <strong>«{deleting.name}»</strong>? Se borrará el archivo de la biblioteca y no se puede deshacer.
          </p>
          <div className="mt-6 flex justify-end gap-2">
            <Button onClick={() => setDeleting(null)}>Cancelar</Button>
            <Button variant="primary" icon={<Trash2 className="size-4" />} onClick={() => void confirmDelete(deleting)}>Eliminar</Button>
          </div>
        </Modal>
      )}
    </div>
  );
}

function EditModal({ song, onClose, onSaved }: { song: Song; onClose: () => void; onSaved: () => Promise<void> }) {
  const { notify } = useToasts();
  const [name, setName] = useState(song.name);
  const [enabled, setEnabled] = useState(song.enabled);
  const [saving, setSaving] = useState(false);
  const trimmed = name.trim();
  const unchanged = trimmed === song.name && enabled === song.enabled;

  async function save() {
    setSaving(true);
    try {
      if (trimmed !== song.name) await api.renameSong(song.id, trimmed);
      if (enabled !== song.enabled) await api.setSongEnabled(song.id, enabled);
      notify("success", "Cambios guardados.");
      await onSaved();
    } catch (e) {
      notify("error", errorMessage(e));
      setSaving(false);
    }
  }

  return (
    <Modal title="Editar canción" onClose={onClose}>
      <form onSubmit={(e) => { e.preventDefault(); if (!unchanged && trimmed) void save(); }} className="space-y-4">
        <label className="block text-sm">
          <span className="mb-1 block text-zinc-400">Nombre</span>
          <input autoFocus value={name} maxLength={120} onChange={(e) => setName(e.target.value)} className="w-full rounded-xl border border-zinc-700 bg-zinc-950 px-3 py-2 outline-none focus:border-red-500" />
        </label>
        <div className="flex items-center justify-between text-sm">
          <span>Activa (puede sonar en una alerta)</span>
          <Switch checked={enabled} onChange={setEnabled} label="Activa" />
        </div>
        <div className="flex justify-end gap-2 pt-2">
          <Button type="button" onClick={onClose}>Cancelar</Button>
          <Button type="submit" variant="primary" loading={saving} disabled={unchanged || !trimmed}>Guardar</Button>
        </div>
      </form>
    </Modal>
  );
}
