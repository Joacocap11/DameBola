import { useCallback, useEffect, useState } from "react";
import {
  AlertTriangle,
  Check,
  Laptop,
  Link2,
  Link2Off,
  Loader2,
  Monitor,
  Plus,
  ShieldCheck,
  Siren,
  Trash2,
  Wifi,
} from "lucide-react";
import { useToasts } from "../components/Toasts";
import { Button, Card, Modal, PageHeader, Switch } from "../components/ui";
import { useNetworkState } from "../hooks/useNetworkState";
import {
  formatRelativeTime,
  isMuted,
  muteUntil,
  osLabel,
  validateCooldown,
  validateHost,
  validatePort,
  type MuteOption,
} from "../lib/network";
import { api, errorMessage } from "../services/api";
import type { DeviceView, NetStatus, NetworkSettings, PairingView } from "../types";

const STATE_LABEL: Record<NetStatus["state"], { text: string; dot: string }> = {
  stopped: { text: "Detenido", dot: "bg-zinc-500" },
  starting: { text: "Iniciando…", dot: "bg-amber-400" },
  running: { text: "Activo", dot: "bg-emerald-400" },
  error: { text: "Con error", dot: "bg-red-500" },
};

function useNow(intervalMs: number): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const id = window.setInterval(() => setNow(Date.now()), intervalMs);
    return () => window.clearInterval(id);
  }, [intervalMs]);
  return now;
}

export function DevicesPage() {
  const { available, loading, status, devices, pairings, settings, saveSettings } = useNetworkState();
  const { notify } = useToasts();
  const now = useNow(5000);
  const [busy, setBusy] = useState<ReadonlySet<string>>(new Set());
  const [sending, setSending] = useState<ReadonlySet<string>>(new Set());
  const [forgetTarget, setForgetTarget] = useState<DeviceView | null>(null);
  const [addOpen, setAddOpen] = useState(false);
  const [dismissed, setDismissed] = useState<string | null>(null);

  const toggle = (set: ReadonlySet<string>, id: string, on: boolean): Set<string> => {
    const next = new Set(set);
    if (on) next.add(id);
    else next.delete(id);
    return next;
  };

  const run = useCallback(
    async (deviceId: string, action: () => Promise<void>) => {
      setBusy((b) => toggle(b, deviceId, true));
      try {
        await action();
      } catch (e) {
        notify("error", errorMessage(e));
      } finally {
        setBusy((b) => toggle(b, deviceId, false));
      }
    },
    [notify],
  );

  async function sendAlert(device: DeviceView) {
    setSending((s) => toggle(s, device.deviceId, true));
    try {
      const r = await api.netSendAlert(device.deviceId);
      // Solo "shown" cuenta como entregada.
      notify(r.status === "shown" ? "success" : r.status === "rejected" ? "warning" : "error", r.message);
    } catch (e) {
      notify("error", errorMessage(e));
    } finally {
      setSending((s) => toggle(s, device.deviceId, false));
    }
  }

  if (loading) {
    return (
      <div className="mx-auto max-w-3xl animate-fade-in">
        <PageHeader title="Dispositivos" />
        <div className="flex items-center gap-3 text-zinc-400">
          <Loader2 className="size-5 animate-spin" /> Cargando dispositivos…
        </div>
      </div>
    );
  }

  if (!available || !status || !settings) {
    return (
      <div className="mx-auto max-w-3xl animate-fade-in">
        <PageHeader title="Dispositivos" subtitle="Conectá tus computadoras para mandarles DAME BOLA." />
        <Card className="flex items-start gap-3 p-5 text-sm text-zinc-300">
          <AlertTriangle className="mt-0.5 size-5 shrink-0 text-amber-400" />
          La red local no está disponible en este momento. DameBola sigue funcionando en modo local.
        </Card>
      </div>
    );
  }

  const paired = devices.filter((d) => d.pairing === "paired");
  const discovered = devices.filter((d) => d.pairing !== "paired" && d.discovered);
  const activePairing = pairings.find((p) => p.state === "active" || `${p.deviceId}:${p.state}` !== dismissed) ?? null;

  return (
    <div className="mx-auto max-w-3xl animate-fade-in space-y-8 pb-8">
      <PageHeader
        title="Dispositivos"
        subtitle="Conectá tus computadoras de la misma red para mandarles DAME BOLA."
        actions={
          <Button variant="primary" icon={<Plus className="size-4" />} onClick={() => setAddOpen(true)}>
            Agregar dispositivo manualmente
          </Button>
        }
      />

      <ThisDevice status={status} />
      <NetworkSettingsCard settings={settings} onSave={saveSettings} now={now} />

      <section>
        <h2 className="mb-3 text-lg font-bold">Dispositivos descubiertos</h2>
        {discovered.length === 0 ? (
          <EmptyState text="No se encontró ningún dispositivo nuevo en tu red. Asegurate de que DameBola esté abierto en el otro equipo, o agregalo manualmente." />
        ) : (
          <div className="space-y-2">
            {discovered.map((d) => (
              <DeviceRow key={d.deviceId} device={d} now={now}>
                <Button
                  variant="primary"
                  icon={<ShieldCheck className="size-4" />}
                  loading={busy.has(d.deviceId)}
                  disabled={d.pairing === "pending"}
                  onClick={() => void run(d.deviceId, () => api.netPair(d.deviceId))}
                >
                  {d.pairing === "pending" ? "Emparejando…" : "Emparejar"}
                </Button>
              </DeviceRow>
            ))}
          </div>
        )}
      </section>

      <section>
        <h2 className="mb-3 text-lg font-bold">Dispositivos emparejados</h2>
        {paired.length === 0 ? (
          <EmptyState text="Todavía no emparejaste ningún dispositivo." />
        ) : (
          <div className="space-y-2">
            {paired.map((d) => {
              const connected = d.connection === "connected";
              return (
                <DeviceRow key={d.deviceId} device={d} now={now}>
                  <Button
                    variant="primary"
                    icon={sending.has(d.deviceId) ? undefined : <Siren className="size-4" />}
                    loading={sending.has(d.deviceId)}
                    disabled={!connected}
                    onClick={() => void sendAlert(d)}
                  >
                    {sending.has(d.deviceId) ? "Enviando…" : "DAME BOLA"}
                  </Button>
                  {connected ? (
                    <Button
                      icon={<Link2Off className="size-4" />}
                      loading={busy.has(d.deviceId)}
                      onClick={() => void run(d.deviceId, () => api.netDisconnect(d.deviceId))}
                    >
                      Desconectar
                    </Button>
                  ) : (
                    <Button
                      icon={<Link2 className="size-4" />}
                      loading={busy.has(d.deviceId) || d.connection === "connecting"}
                      onClick={() => void run(d.deviceId, () => api.netConnect(d.deviceId))}
                    >
                      {d.connection === "connecting" ? "Conectando…" : "Conectar"}
                    </Button>
                  )}
                  <Button variant="danger" icon={<Trash2 className="size-4" />} onClick={() => setForgetTarget(d)}>
                    Olvidar
                  </Button>
                </DeviceRow>
              );
            })}
          </div>
        )}
      </section>

      {addOpen && <AddManualModal onClose={() => setAddOpen(false)} />}

      {forgetTarget && (
        <Modal title="Olvidar dispositivo" onClose={() => setForgetTarget(null)}>
          <p className="text-sm text-zinc-300">
            ¿Querés olvidar a <strong>{forgetTarget.name}</strong>? Se va a desconectar y vas a tener que emparejarlo de nuevo para volver a usarlo.
          </p>
          <div className="mt-6 flex justify-end gap-2">
            <Button variant="ghost" onClick={() => setForgetTarget(null)}>
              Cancelar
            </Button>
            <Button
              variant="danger"
              onClick={() => {
                const target = forgetTarget;
                setForgetTarget(null);
                void run(target.deviceId, () => api.netForget(target.deviceId));
              }}
            >
              Olvidar
            </Button>
          </div>
        </Modal>
      )}

      {activePairing && (
        <PairingModal
          pairing={activePairing}
          onClose={() => setDismissed(`${activePairing.deviceId}:${activePairing.state}`)}
        />
      )}
    </div>
  );
}

function EmptyState({ text }: { text: string }) {
  return <Card className="border-dashed p-5 text-sm text-zinc-400">{text}</Card>;
}

function DeviceIcon({ os }: { os: DeviceView["os"] }) {
  const Icon = os === "other" ? Monitor : Laptop;
  return (
    <div className="grid size-11 shrink-0 place-items-center rounded-xl bg-zinc-800 text-zinc-300">
      <Icon className="size-5" />
    </div>
  );
}

function ConnectionBadge({ device }: { device: DeviceView }) {
  if (device.pairing !== "paired") return null;
  const map = {
    connected: { text: "Conectado", cls: "text-emerald-400", dot: "bg-emerald-400" },
    connecting: { text: "Conectando…", cls: "text-amber-400", dot: "bg-amber-400" },
    disconnected: { text: "Desconectado", cls: "text-zinc-400", dot: "bg-zinc-500" },
  }[device.connection];
  return (
    <span className={`inline-flex items-center gap-1.5 text-xs font-semibold ${map.cls}`}>
      <span className={`size-2 rounded-full ${map.dot}`} />
      {map.text}
    </span>
  );
}

function DeviceRow({ device, now, children }: { device: DeviceView; now: number; children: React.ReactNode }) {
  const where = device.addresses.length > 0 ? `${device.addresses[0]}${device.port ? `:${device.port}` : ""}` : null;
  return (
    <Card className="flex flex-wrap items-center gap-4 p-4">
      <DeviceIcon os={device.os} />
      <div className="min-w-0 flex-1">
        <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
          <span className="truncate font-semibold">{device.name}</span>
          <ConnectionBadge device={device} />
        </div>
        <p className="mt-0.5 text-xs text-zinc-500">
          {osLabel(device.os)}
          {where && ` · ${where}`}
          {device.connection === "connected"
            ? " · activo ahora"
            : device.lastSeenMs !== null && ` · visto ${formatRelativeTime(device.lastSeenMs, now)}`}
        </p>
        {device.lastError && <p className="mt-1 text-xs text-amber-400">{device.lastError}</p>}
      </div>
      <div className="flex flex-wrap items-center gap-2">{children}</div>
    </Card>
  );
}

function ThisDevice({ status }: { status: NetStatus }) {
  const st = STATE_LABEL[status.state];
  return (
    <Card className="p-5">
      <div className="flex items-start gap-4">
        <DeviceIcon os={status.os} />
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
            <h2 className="text-lg font-bold">{status.deviceName}</h2>
            <span className="inline-flex items-center gap-1.5 text-xs font-semibold text-zinc-300">
              <span className={`size-2 rounded-full ${st.dot}`} />
              {st.text}
            </span>
            <span className="text-xs text-zinc-500">Este equipo · {osLabel(status.os)}</span>
          </div>
          {status.error && <p className="mt-1 text-sm text-red-400">{status.error}</p>}
          <dl className="mt-3 grid grid-cols-[auto_1fr] gap-x-4 gap-y-1.5 text-sm">
            <dt className="text-zinc-500">Direcciones</dt>
            <dd className="text-zinc-200">{status.addresses.length > 0 ? status.addresses.join(", ") : "Sin red local detectada"}</dd>
            <dt className="text-zinc-500">Puerto</dt>
            <dd className="text-zinc-200">{status.port}</dd>
            <dt className="text-zinc-500">Huella</dt>
            <dd className="break-all font-mono text-xs text-zinc-200">{status.fingerprint}</dd>
            <dt className="text-zinc-500">Descubrimiento</dt>
            <dd className="text-zinc-200">
              {status.mdns === "active" && "Automático (mDNS) activo"}
              {status.mdns === "disabled" && "Automático desactivado"}
              {status.mdns === "error" && (
                <span className="text-amber-400">
                  No disponible{status.mdnsError ? `: ${status.mdnsError}` : ""}. Podés agregar dispositivos manualmente.
                </span>
              )}
            </dd>
          </dl>
          <p className="mt-3 flex items-start gap-2 text-xs text-zinc-500">
            <ShieldCheck className="mt-0.5 size-3.5 shrink-0" />
            {status.keyStorage === "keychain"
              ? "La clave privada de este equipo se guarda en el llavero del sistema."
              : "La clave privada de este equipo se guarda en un archivo protegido dentro de la carpeta de datos de DameBola."}
          </p>
        </div>
      </div>
    </Card>
  );
}

function NetworkSettingsCard({
  settings,
  onSave,
  now,
}: {
  settings: NetworkSettings;
  onSave: (s: NetworkSettings) => Promise<boolean>;
  now: number;
}) {
  const [port, setPort] = useState(String(settings.port));
  const [cooldown, setCooldown] = useState(String(settings.alertCooldownSecs));
  const [saving, setSaving] = useState(false);

  // Si el backend cambia los valores (guardado, otro origen), el borrador se resincroniza.
  useEffect(() => setPort(String(settings.port)), [settings.port]);
  useEffect(() => setCooldown(String(settings.alertCooldownSecs)), [settings.alertCooldownSecs]);

  const portError = validatePort(port);
  const cooldownError = validateCooldown(cooldown);
  const dirty = port.trim() !== String(settings.port) || cooldown.trim() !== String(settings.alertCooldownSecs);
  const muted = isMuted(settings.mutedUntilMs, now);

  async function patch(p: Partial<NetworkSettings>) {
    setSaving(true);
    await onSave({ ...settings, ...p });
    setSaving(false);
  }

  const mute = (o: MuteOption) => void patch({ mutedUntilMs: muteUntil(o, Date.now()) });

  return (
    <Card className="divide-y divide-zinc-800">
      <div className="flex items-center justify-between gap-4 p-5">
        <div>
          <h3 className="font-semibold">Red local</h3>
          <p className="text-sm text-zinc-400">Permite descubrir y conectarse con otros equipos de tu red.</p>
        </div>
        <Switch label="Activar red local" checked={settings.enabled} disabled={saving} onChange={(v) => void patch({ enabled: v })} />
      </div>

      <div className="flex flex-wrap items-end gap-4 p-5">
        <label className="text-sm">
          <span className="mb-1 block text-zinc-400">Puerto</span>
          <input
            inputMode="numeric"
            value={port}
            onChange={(e) => setPort(e.target.value)}
            aria-invalid={portError !== null}
            className="w-32 rounded-xl border border-zinc-700 bg-zinc-950 px-3 py-2 outline-none focus:border-red-500"
          />
        </label>
        <label className="text-sm">
          <span className="mb-1 block text-zinc-400">Espera entre alertas (segundos)</span>
          <input
            inputMode="numeric"
            value={cooldown}
            onChange={(e) => setCooldown(e.target.value)}
            aria-invalid={cooldownError !== null}
            className="w-32 rounded-xl border border-zinc-700 bg-zinc-950 px-3 py-2 outline-none focus:border-red-500"
          />
        </label>
        <Button
          variant="primary"
          icon={<Check className="size-4" />}
          loading={saving}
          disabled={!dirty || portError !== null || cooldownError !== null}
          onClick={() => void patch({ port: Number(port), alertCooldownSecs: Number(cooldown) })}
        >
          Guardar
        </Button>
        {dirty && (portError ?? cooldownError) && (
          <p className="w-full text-xs text-red-400">{portError ?? cooldownError}</p>
        )}
      </div>

      <div className="flex items-center justify-between gap-4 p-5">
        <div>
          <h3 className="font-semibold">Recibir alertas de otros equipos</h3>
          <p className="text-sm text-zinc-400">Si lo desactivás, nadie puede mostrarte DAME BOLA.</p>
        </div>
        <Switch
          label="Recibir alertas remotas"
          checked={settings.remoteAlertsEnabled}
          disabled={saving}
          onChange={(v) => void patch({ remoteAlertsEnabled: v })}
        />
      </div>

      <div className="p-5">
        <h3 className="font-semibold">Silenciar alertas remotas</h3>
        <p className="mb-3 text-sm text-zinc-400">
          {muted && settings.mutedUntilMs !== null
            ? `Silenciado hasta ${new Date(settings.mutedUntilMs).toLocaleString("es", { dateStyle: "short", timeStyle: "short" })}.`
            : "Las alertas remotas no están silenciadas."}
        </p>
        <div className="flex flex-wrap gap-2">
          <Button disabled={saving} onClick={() => mute("15m")}>15 minutos</Button>
          <Button disabled={saving} onClick={() => mute("1h")}>1 hora</Button>
          <Button disabled={saving} onClick={() => mute("tomorrow")}>Hasta mañana</Button>
          <Button variant="ghost" disabled={saving || settings.mutedUntilMs === null} onClick={() => void patch({ mutedUntilMs: null })}>
            Quitar silencio
          </Button>
        </div>
      </div>
    </Card>
  );
}

function AddManualModal({ onClose }: { onClose: () => void }) {
  const { notify } = useToasts();
  const [host, setHost] = useState("");
  const [port, setPort] = useState("38473");
  const [touched, setTouched] = useState(false);
  const [adding, setAdding] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const hostError = validateHost(host);
  const portError = validatePort(port);

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    setTouched(true);
    if (hostError || portError) return;
    setAdding(true);
    setError(null);
    try {
      const d = await api.netAddManual(host.trim(), Number(port));
      notify("success", `Se encontró a ${d.name}.`);
      onClose();
    } catch (err) {
      setError(errorMessage(err));
      setAdding(false);
    }
  }

  return (
    <Modal title="Agregar dispositivo manualmente" onClose={adding ? () => undefined : onClose}>
      <form onSubmit={(e) => void submit(e)} className="space-y-4">
        <label className="block text-sm">
          <span className="mb-1 block text-zinc-400">Dirección IP</span>
          <input
            autoFocus
            value={host}
            onChange={(e) => setHost(e.target.value)}
            placeholder="192.168.1.20"
            className="w-full rounded-xl border border-zinc-700 bg-zinc-950 px-3 py-2 outline-none focus:border-red-500"
          />
          {touched && hostError && <span className="mt-1 block text-xs text-red-400">{hostError}</span>}
        </label>
        <label className="block text-sm">
          <span className="mb-1 block text-zinc-400">Puerto</span>
          <input
            inputMode="numeric"
            value={port}
            onChange={(e) => setPort(e.target.value)}
            className="w-full rounded-xl border border-zinc-700 bg-zinc-950 px-3 py-2 outline-none focus:border-red-500"
          />
          {touched && portError && <span className="mt-1 block text-xs text-red-400">{portError}</span>}
        </label>
        {error && <p className="flex items-start gap-2 text-sm text-red-400"><AlertTriangle className="mt-0.5 size-4 shrink-0" />{error}</p>}
        <div className="flex justify-end gap-2 pt-2">
          <Button type="button" variant="ghost" disabled={adding} onClick={onClose}>
            Cancelar
          </Button>
          <Button type="submit" variant="primary" loading={adding} icon={adding ? undefined : <Wifi className="size-4" />}>
            {adding ? "Buscando…" : "Agregar"}
          </Button>
        </div>
      </form>
    </Modal>
  );
}

function PairingModal({ pairing, onClose }: { pairing: PairingView; onClose: () => void }) {
  const { notify } = useToasts();
  const [deciding, setDeciding] = useState(false);
  const active = pairing.state === "active";
  const responder = pairing.role === "responder";

  async function decide(accept: boolean) {
    setDeciding(true);
    try {
      await api.netPairingDecision(pairing.deviceId, accept);
    } catch (e) {
      notify("error", errorMessage(e));
    } finally {
      setDeciding(false);
    }
  }

  if (!active) {
    const ok = pairing.state === "completed";
    const text =
      pairing.message ??
      (ok
        ? `${pairing.name} quedó emparejado.`
        : pairing.state === "expired"
          ? "El emparejamiento expiró. Probá de nuevo."
          : "El emparejamiento fue rechazado.");
    return (
      <Modal title={ok ? "Emparejamiento completo" : "Emparejamiento no completado"} onClose={onClose}>
        <p className={`text-sm ${ok ? "text-emerald-300" : "text-amber-300"}`}>{text}</p>
        <div className="mt-6 flex justify-end">
          <Button variant="primary" onClick={onClose}>
            Cerrar
          </Button>
        </div>
      </Modal>
    );
  }

  const waitingPeer = pairing.localDecision === "accepted" && pairing.peerDecision === "pending";
  return (
    <Modal title={`Emparejar con ${pairing.name}`} onClose={onClose} locked>
      <p className="text-sm text-zinc-300">Verificá que el otro equipo muestre exactamente el mismo código.</p>
      <div className="my-5 rounded-2xl border border-zinc-700 bg-zinc-950 py-5 text-center font-mono text-5xl font-black tracking-widest text-red-400" aria-label={`Código ${pairing.code}`}>
        {pairing.code}
      </div>
      {pairing.message && <p className="mb-3 text-sm text-zinc-400">{pairing.message}</p>}
      {waitingPeer ? (
        <div className="flex items-center gap-3 text-sm text-zinc-300">
          <Loader2 className="size-4 animate-spin" /> Esperando que {pairing.name} confirme…
        </div>
      ) : (
        pairing.localDecision === "pending" && pairing.peerDecision === "accepted" && (
          <p className="mb-3 text-sm text-emerald-300">{pairing.name} ya confirmó el código.</p>
        )
      )}
      <div className="mt-4 flex flex-wrap justify-end gap-2">
        <Button variant="danger" disabled={deciding} onClick={() => void decide(false)}>
          No coincide, cancelar
        </Button>
        {pairing.localDecision === "pending" && (
          <Button variant="primary" loading={deciding} onClick={() => void decide(true)}>
            {responder ? "Aceptar emparejamiento" : "El código coincide"}
          </Button>
        )}
      </div>
    </Modal>
  );
}
