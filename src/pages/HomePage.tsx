import { useState } from "react";
import { BellRing, Music2, Radio, Siren } from "lucide-react";
import { ChickenDance } from "../components/ChickenDance";
import { Button, Card } from "../components/ui";
import { useToasts } from "../components/Toasts";
import { useAppState } from "../hooks/useAppState";
import { useNetworkState } from "../hooks/useNetworkState";
import { countPlayable } from "../lib/songs";
import { pluralSongs } from "../lib/format";
import { api, errorMessage } from "../services/api";

export function HomePage({ onGoToSongs }: { onGoToSongs: () => void }) {
  const { songs, alertActive, settings } = useAppState();
  const { devices } = useNetworkState();
  const { notify } = useToasts();
  const connected = devices.filter((d) => d.connection === "connected");
  const badge =
    connected.length === 0
      ? "Modo local"
      : connected.length === 1
        ? `Conectado a ${connected[0]?.name ?? "otro equipo"}`
        : `${connected.length} dispositivos conectados`;
  const [starting, setStarting] = useState(false);

  const playable = songs ? countPlayable(songs) : 0;
  const total = songs?.length ?? 0;

  async function trigger() {
    setStarting(true);
    try {
      const r = await api.triggerAlert();
      if (r.audioError) notify("warning", `La alerta se mostró sin audio: ${r.audioError}`);
      else if (!r.songName) notify("info", "Alerta sin audio: no hay canciones activas.");
      else notify("success", `Alerta en ${r.monitors} monitor${r.monitors === 1 ? "" : "es"} · ♪ ${r.songName}`);
    } catch (e) {
      notify("error", errorMessage(e));
    } finally {
      setStarting(false);
    }
  }

  return (
    <div className="mx-auto flex min-h-full max-w-3xl flex-col items-center justify-center gap-8 py-6 text-center animate-fade-in">
      <div className="relative">
        <div className="absolute inset-0 -z-10 rounded-full bg-red-600/20 blur-3xl" />
        <ChickenDance className="size-52" />
      </div>

      <div>
        <h1 className="text-6xl font-black tracking-tight">
          Dame<span className="text-red-500">Bola</span>
        </h1>
        <div className="mt-3 inline-flex items-center gap-2 rounded-full border border-zinc-700 bg-zinc-900 px-3 py-1 text-sm text-zinc-300">
          <Radio className="size-4 text-emerald-400" />
          {badge}
        </div>
      </div>

      <Card className="flex items-center gap-4 px-5 py-3">
        <Music2 className="size-5 text-red-400" />
        <span className="text-sm text-zinc-300">
          {songs === null ? "Cargando canciones…" : `${pluralSongs(playable)} disponible${playable === 1 ? "" : "s"}`}
          {songs !== null && total !== playable && <span className="text-zinc-500"> · {total} en total</span>}
        </span>
        {songs !== null && playable === 0 && (
          <button onClick={onGoToSongs} className="text-sm font-semibold text-red-400 hover:underline">
            Agregar canciones
          </button>
        )}
      </Card>

      <Button
        variant="primary"
        onClick={trigger}
        loading={starting}
        disabled={alertActive}
        icon={alertActive ? <BellRing className="size-7" /> : <Siren className="size-7" />}
        className="rounded-2xl px-10 py-5 text-2xl font-black tracking-wide animate-pulse-ring"
      >
        {alertActive ? "ALERTA EN CURSO…" : "PROBAR DAME BOLA"}
      </Button>

      <p className="max-w-xl text-sm leading-relaxed text-zinc-400">
        DameBola muestra una alerta a pantalla completa en todos tus monitores: fondo rojo, un pollo bailando, un texto
        gigante y una canción al azar de tu biblioteca. Se cierra con el botón, con Escape o sola tras unos segundos, y
        tus aplicaciones siguen funcionando como siempre.
      </p>
      {settings?.shortcutEnabled && <p className="text-sm text-zinc-300"><kbd className="rounded border border-zinc-600 px-2 py-1 font-mono">F12</kbd> envía DAME BOLA al compañero de Configuración, con esta ventana en primer plano.</p>}
    </div>
  );
}
