# Arquitectura (Fase 1)

```
src/                     Frontend (React + TS + Tailwind)
  pages/                 Inicio, Canciones, Configuración (solo UI)
  components/            Sidebar, ChickenDance, Modal, Toasts…
  alert/AlertWindow.tsx  Vista de las ventanas de alerta
  hooks/useAppState.tsx  Estado global (canciones, ajustes, reproducción, alerta)
  lib/                   Lógica pura y probada (filtrar/ordenar, formato)
  services/api.ts        ÚNICA capa que llama a `invoke`
  services/events.ts     Suscripción a eventos del backend
  services/transport.ts  Contratos (solo tipos) para fases con red
src-tauri/src/           Backend (Rust)
  db.rs                  SQLite: apertura, migraciones por PRAGMA user_version
  songs.rs               ABM: importar (copia + SHA-256 + validación), editar, borrar
  selection.rs           Selección aleatoria pura (sin I/O)
  settings.rs            Ajustes validados, persistidos en SQLite
  audio.rs               Motor de audio único (hilo dedicado, rodio)
  alert.rs               Ventanas por monitor + ciclo de vida de la alerta
  commands.rs            Comandos Tauri (capa fina)
  lib.rs                 Composición (`Services`) y registro
```

## Decisiones

- **Un solo motor de audio, en Rust.** Las ventanas de alerta no reproducen nada; así el audio suena una vez sin importar cuántos monitores haya, y vista previa y alerta comparten el mismo reproductor (nunca suenan dos pistas a la vez). Un hilo es dueño del stream (no es `Send`) y recibe comandos por un canal; notifica cambios con el evento `playback://status`.
- **Una ventana por monitor, mismo bundle.** Las ventanas `alert-N` cargan `index.html`; `main.tsx` decide por la etiqueta de la ventana qué renderizar. Piden su contexto (`get_alert_context`) al montarse.
- **Ciclo de vida idempotente.** `alert::end` se llama desde el botón, Escape, el temporizador (con número de generación para no cerrar una alerta nueva) o el cierre manual de una ventana (`Destroyed`). Hay una sola alerta activa (`start` falla si ya existe).
- **IDs UUID + hash SHA-256 único** en `songs`: base para sincronizar entre equipos sin colisiones. Fechas en milisegundos UTC. Migraciones versionadas.
- **Importación segura:** copia a `<uuid>.part` calculando el hash, valida firma y decodifica una muestra sobre la copia, comprueba duplicados, renombra y registra; ante cualquier error se limpia el temporal.
- **Seguridad:** sin permisos de filesystem en el frontend; diálogo de archivos y todas las rutas viven en Rust.
- **Futuro (no implementado):** `AlertRequest` en `alert.rs` es el punto de entrada para una alerta remota; `services/transport.ts` define los contratos de WebSocket/sincronización.
