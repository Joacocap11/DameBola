import { createContext, useCallback, useContext, useMemo, useRef, useState, type ReactNode } from "react";
import { AlertTriangle, CheckCircle2, Info, XCircle } from "lucide-react";

type ToastKind = "success" | "error" | "warning" | "info";
interface Toast {
  id: number;
  kind: ToastKind;
  message: string;
}

interface ToastApi {
  notify: (kind: ToastKind, message: string) => void;
}

const ToastContext = createContext<ToastApi | null>(null);

export function useToasts(): ToastApi {
  const ctx = useContext(ToastContext);
  if (!ctx) throw new Error("useToasts debe usarse dentro de <ToastProvider>");
  return ctx;
}

const styles: Record<ToastKind, { box: string; icon: ReactNode }> = {
  success: { box: "border-emerald-800 bg-emerald-950/90 text-emerald-100", icon: <CheckCircle2 className="size-5 shrink-0 text-emerald-400" /> },
  error: { box: "border-red-800 bg-red-950/90 text-red-100", icon: <XCircle className="size-5 shrink-0 text-red-400" /> },
  warning: { box: "border-amber-800 bg-amber-950/90 text-amber-100", icon: <AlertTriangle className="size-5 shrink-0 text-amber-400" /> },
  info: { box: "border-zinc-700 bg-zinc-900/95 text-zinc-100", icon: <Info className="size-5 shrink-0 text-zinc-400" /> },
};

export function ToastProvider({ children }: { children: ReactNode }) {
  const [toasts, setToasts] = useState<Toast[]>([]);
  const nextId = useRef(1);

  const notify = useCallback((kind: ToastKind, message: string) => {
    const id = nextId.current++;
    setToasts((t) => [...t.slice(-3), { id, kind, message }]);
    window.setTimeout(() => setToasts((t) => t.filter((x) => x.id !== id)), kind === "error" ? 8000 : 4500);
  }, []);

  const api = useMemo(() => ({ notify }), [notify]);

  return (
    <ToastContext.Provider value={api}>
      {children}
      <div className="pointer-events-none fixed bottom-4 right-4 z-[60] flex w-96 max-w-[calc(100vw-2rem)] flex-col gap-2" aria-live="polite">
        {toasts.map((t) => (
          <div key={t.id} className={`pointer-events-auto flex items-start gap-3 rounded-xl border px-4 py-3 text-sm shadow-xl animate-pop-in ${styles[t.kind].box}`}>
            {styles[t.kind].icon}
            <span className="whitespace-pre-line">{t.message}</span>
          </div>
        ))}
      </div>
    </ToastContext.Provider>
  );
}
