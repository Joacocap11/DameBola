# Atajo de teclado para enviar DAME BOLA

Un atajo configurable envía una alerta remota. Por defecto es **F12** y solo funciona con la **ventana principal** de DameBola enfocada (alcance **local**). Desde la Fase 2.6 se puede elegir el alcance **global**: el sistema operativo avisa a DameBola de esa combinación aunque la ventana esté oculta, minimizada u otra aplicación tenga el foco. **El modo global nunca se activa solo**: hay que elegirlo en Configuración.

## Cambiar el atajo

En **Configuración → Combinación**:

1. Pulsá **Cambiar atajo**. Aparece «Presioná la tecla o combinación…».
2. Presioná la tecla (por ejemplo `F8`) o la combinación (por ejemplo `Ctrl + Shift + B`). Se muestra como **Nuevo atajo**.
3. **Guardar** lo aplica al instante (y lo persiste); **Cancelar** conserva el anterior. **Esc** también cancela la captura.
4. **Restablecer F12** vuelve al valor predeterminado.

Guardar el atajo no toca el resto de los ajustes ni tus cambios sin guardar en otros campos. El interruptor **Atajo de teclado** (y el destinatario) se guardan con **Guardar cambios**, como antes. Inicio muestra el atajo configurado.

Durante la captura **no se envía ninguna alerta** (ni siquiera con el atajo actual); se suspende el envío y todas las teclas quedan consumidas por la captura. Se ignoran las pulsaciones repetidas, la composición de texto (IME) y los modificadores solos.

## Qué combinaciones se aceptan

| Permitido | Rechazado |
|---|---|
| Teclas de función **F1–F24**, solas o con Ctrl, Alt, Shift o Cmd/Win. | `Alt + F4` (cierra la ventana). |
| Letra (A–Z) o número (0–9) **con Ctrl o Cmd** (más Shift/Alt opcionales). | Letras o números sin Ctrl/Cmd (se dispararía al escribir), p. ej. `B`, `Shift + B`, `Alt + B`. |
| | `Ctrl + Alt + …` con letras/números (en muchos teclados es AltGr y se usa para escribir). |
| | `Ctrl/Cmd + A, C, V, X, Z, Y, W, Q, R, N, T` (editar, cerrar, recargar), con o sin Shift. |
| | Teclas de edición/navegación y Escape: Backspace, Delete, Enter, Tab, Espacio, flechas, Inicio/Fin, teclado numérico, símbolos. |

Las letras se identifican por la **tecla física** (`Ctrl + Shift + B` funciona igual con otras distribuciones, y con Option en macOS no cambia a «∫»). Formato guardado: `F8`, `Ctrl+Shift+B` (orden fijo Ctrl, Alt, Shift, Meta). El **backend valida y normaliza siempre** (`src-tauri/src/shortcut.rs`); la interfaz solo da respuesta inmediata, con los mismos vectores de prueba en ambos lados. Un valor guardado no válido vuelve a F12.

En alcance local, el atajo actúa solo dentro de DameBola. En teclados con teclas multimedia puede hacer falta `Fn`. `Win/Cmd + …` puede ser interceptado por el sistema operativo antes de llegar a la app.

## Compatibilidad con instalaciones anteriores

Los ajustes de la versión con F12 fijo no tienen `shortcutKey`: al cargarlos se asume **F12** y se conservan el estado activado/desactivado y el destinatario. No se modifican claves, certificados, canciones ni emparejamientos.

## Envío y destinatario (sin cambios)

- Envía desde el backend Rust (`send.rs` → `NetworkService::send_alert`, el mismo camino que `netSendAlert`); el botón DAME BOLA de Dispositivos usa ese mismo servicio; el botón PROBAR DAME BOLA de Inicio sigue siendo una prueba local.
- Destinatario: el seleccionado, o el único dispositivo emparejado. Con varios exige elegir uno aunque solo uno esté conectado.
- Nunca cambia a otro destinatario si el seleccionado se desconecta o se olvida: avisa.
- Ignora la repetición de tecla y las pulsaciones mientras hay un envío pendiente (un cerrojo común al atajo local, al global y a la bandeja).
- Conserva los límites y permisos de recepción del destinatario (una alerta cada 15 s, silencio, desactivar recepción). Solo se informa éxito con la confirmación `shown`.

## Alcance del atajo: local o global

**Configuración → Alcance del atajo**:

| Alcance | Quién escucha la tecla | Funciona con |
|---|---|---|
| Solo dentro de DameBola (por defecto) | La ventana principal (React). | Ventana principal enfocada. |
| Global | El sistema operativo, vía `tauri-plugin-global-shortcut`; el backend Rust envía. | Ventana visible, minimizada, oculta u otra app enfocada. No funciona si salís completamente. |

- Los ajustes existentes (combinación, activado/desactivado, destinatario) se reutilizan; no hay un segundo sistema de destinatarios. Las instalaciones previas quedan en **local**; F12 no se registra como global salvo que lo elijas.
- Solo se registra **una combinación concreta** mediante la API del sistema. No se lee ni se guarda ninguna otra tecla (no hay keylogger).
- Con alcance global el manejador de la ventana se desactiva: una pulsación nunca produce dos alertas. Se descarta la repetición de tecla (`PressGate`: una pulsación hasta que se suelta; si el sistema no informa la liberación, vuelve a valer tras 1,5 s) y los envíos simultáneos (`SendGuard`). La frecuencia y la autorización las sigue imponiendo el receptor.
- Al **guardar** una combinación o un cambio de alcance: 1) se valida; 2) se registra la nueva; 3) si funciona, se libera la anterior; 4) se guarda en SQLite; 5) si algo falla, queda la anterior (y se deshacen los pasos hechos). Si el registro falla se muestra el error y la configuración guardada no cambia.
- Si al iniciar la app el atajo global no se puede registrar (otra app lo ocupó), la app arranca igual, el modo global queda inactivo y Configuración muestra el motivo. Guardar otros ajustes no queda bloqueado por eso.
- Al salir completamente se liberan los atajos (`unregister_all`).

### Combinaciones reservadas (modo global)

Además de las reglas generales:

| Sistema | Rechazado en modo global |
|---|---|
| Windows | Cualquier combinación con la tecla Windows (`Win + …`), `Ctrl + Shift + F10`. |
| macOS | `Cmd + Q/H/M/W/F`, `Cmd + Shift + Q/3/4/5/6`, `Ctrl + Cmd + Q`, `Cmd + Opt + H/M/D`. |

Una tecla de función sola (p. ej. F12) se acepta pero con una advertencia: se la quita a todas las demás aplicaciones. Ejemplos recomendados: **Ctrl + Shift + B** (Windows) y **Cmd + Shift + B** (macOS).

### Limitaciones del modo global

- Si otra aplicación ya usa la combinación, Windows lo informa (`RegisterHotKey` falla) y DameBola lo muestra. **En macOS el sistema puede permitir registrar la misma combinación en dos programas sin error**: la comprobación de «ocupado» no es fiable allí (observado con dos instancias de DameBola). Elegí una combinación poco común.
- No se muestran notificaciones nativas: con la ventana oculta el resultado de un envío no tiene un aviso propio (si la ventana está visible se muestra un aviso). Se ve en el equipo destinatario.
- No probado con equipos físicos todavía; ver `PHASE2_BACKGROUND_STATUS.md`.

## Pruebas

Automáticas: `npm test` (`src/lib/shortcut.test.ts`: validación y normalización, evento→atajo, captura, cancelación, no envío durante la captura, sin doble envío al pasar a global, avisos de combinaciones reservadas) y `npm run test:rust` (`shortcut.rs`, `settings.rs`, `hotkey.rs`, `send.rs`: validación, reglas por sistema, registro transaccional con atajo ocupado, cambio de atajo, repetición de tecla, destinatario, envíos simultáneos, persistencia, ajustes anteriores).

(Fase 2.5) Comprobado en un navegador Chromium real contra la interfaz (con la API de Tauri simulada): captura con F12 sin enviar, teclas inválidas, Esc, guardar `Ctrl + Shift + B`, la combinación anterior deja de enviar y la nueva envía, restablecer F12, resto de ajustes intactos.

## Verificación manual en la app instalada

1. Configuración → Cambiar atajo → `F8` → Guardar. F8 envía; F12 ya no.
2. Cambiar atajo y presionar F12 (o la combinación actual): no envía; Esc cancela y deja el anterior.
3. Probar `B`, `Backspace`, `Ctrl + C`, `Alt + F4`: se rechazan con un motivo.
4. Guardar `Ctrl + Shift + B`, cerrar y reabrir la app: se conserva y Inicio lo muestra.
5. Restablecer F12. Desactivar el atajo y guardar: no envía.
6. Con dos equipos emparejados, comprobar destinatario obligatorio; desconectar u olvidar el elegido: no se envía a otro.
7. Mantener la tecla apretada: una sola solicitud. Con la app minimizada u otra aplicación al frente: no envía.

## Verificación manual del modo global (pendiente en equipos físicos)

1. Configuración → Alcance → Global, combinación `Ctrl + Shift + B` (Windows) o `Cmd + Shift + B` (macOS) → Guardar. Debe aparecer «Registrado en el sistema».
2. Con otra aplicación al frente, pulsar la combinación: el equipo destino muestra la alerta.
3. Cerrar la ventana (queda en la bandeja) y repetir. Minimizada y visible también.
4. Mantener la tecla apretada: una sola alerta.
5. Probar `Win + B` en Windows o `Cmd + Q` en macOS: se rechaza con un motivo.
6. Cambiar a una combinación ocupada por otro programa (Windows): error y sigue la anterior.
7. Volver a «Solo dentro de DameBola»: la combinación deja de funcionar con otra app al frente.
8. Salir completamente desde la bandeja: la combinación ya no hace nada.
