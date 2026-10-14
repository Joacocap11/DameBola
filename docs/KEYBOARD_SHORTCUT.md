# Atajo de teclado para enviar DAME BOLA

Un atajo configurable envía una alerta remota mientras la **ventana principal** de DameBola tiene el foco. Por defecto es **F12**. Sirve desde Inicio, Canciones, Dispositivos o Configuración. No funciona con la app minimizada o cerrada, con otra aplicación en primer plano ni con una ventana de alerta enfocada. **No es un atajo global**: no se registra en el sistema operativo.

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

Windows reserva F12 para atajos globales (depurador), pero este atajo actúa solo dentro de DameBola. En teclados con teclas multimedia puede hacer falta `Fn`. `Win/Cmd + …` puede ser interceptado por el sistema operativo antes de llegar a la app.

## Compatibilidad con instalaciones anteriores

Los ajustes de la versión con F12 fijo no tienen `shortcutKey`: al cargarlos se asume **F12** y se conservan el estado activado/desactivado y el destinatario. No se modifican claves, certificados, canciones ni emparejamientos.

## Envío y destinatario (sin cambios)

- Envía por `netSendAlert`, igual que el botón DAME BOLA de Dispositivos; el botón PROBAR DAME BOLA de Inicio sigue siendo una prueba local.
- Destinatario: el seleccionado, o el único dispositivo emparejado. Con varios exige elegir uno aunque solo uno esté conectado.
- Nunca cambia a otro destinatario si el seleccionado se desconecta o se olvida: avisa.
- Ignora la repetición de tecla y las pulsaciones mientras hay un envío pendiente.
- Conserva los límites y permisos de recepción del destinatario (una alerta cada 15 s, silencio, desactivar recepción). Solo se informa éxito con la confirmación `shown`.

## Pruebas

Automáticas: `npm test` (`src/lib/shortcut.test.ts`: validación y normalización, evento→atajo, captura, cancelación, no envío durante la captura, sustitución del atajo, destinatario) y `npm run test:rust` (`shortcut.rs` y `settings.rs`: validación, normalización, persistencia, ajustes anteriores).

Comprobado en un navegador Chromium real contra la interfaz (con la API de Tauri simulada): captura con F12 sin enviar, teclas inválidas, Esc, guardar `Ctrl + Shift + B`, la combinación anterior deja de enviar y la nueva envía, restablecer F12, resto de ajustes intactos.

## Verificación manual en la app instalada

1. Configuración → Cambiar atajo → `F8` → Guardar. F8 envía; F12 ya no.
2. Cambiar atajo y presionar F12 (o la combinación actual): no envía; Esc cancela y deja el anterior.
3. Probar `B`, `Backspace`, `Ctrl + C`, `Alt + F4`: se rechazan con un motivo.
4. Guardar `Ctrl + Shift + B`, cerrar y reabrir la app: se conserva y Inicio lo muestra.
5. Restablecer F12. Desactivar el atajo y guardar: no envía.
6. Con dos equipos emparejados, comprobar destinatario obligatorio; desconectar u olvidar el elegido: no se envía a otro.
7. Mantener la tecla apretada: una sola solicitud. Con la app minimizada u otra aplicación al frente: no envía.
