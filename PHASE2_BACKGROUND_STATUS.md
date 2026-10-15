# Estado de la Fase 2.6 (segundo plano y atajos globales)

Rama: `feature/phase2-background`, creada desde `feature/phase2-stability` (Fase 2.5). Cadena de PR: `main` ← #1 (`phase2-p2p`) ← … ← #3 (`phase2-stability`) ← este PR. Ningún PR anterior está fusionado y este tampoco se fusiona automáticamente. Versión de prueba: **0.1.4**.

## Implementado

- **Aplicación residente**: cerrar la ventana principal la oculta (con «Mantener DameBola ejecutándose al cerrar la ventana», activado por defecto, y bandeja activa). Red TLS/WebSocket, mDNS, emparejamientos, recepción y envío siguen funcionando. Sin ventanas invisibles extra: solo la principal se oculta; las de alerta se crean y destruyen como antes y no abren la principal.
- **Una instancia**: `tauri-plugin-single-instance` (un segundo lanzamiento muestra la ventana existente) + bloqueo por carpeta de datos.
- **Bandeja / barra de menús** (icono de DameBola): estado de conexión, destinatario, Abrir DameBola (muestra, restaura, trae al frente y enfoca), Enviar DAME BOLA (deshabilitado sin destinatario válido y conectado; el texto explica por qué), Silenciar/Reactivar alertas (1 h), Iniciar con el sistema (casilla), Salir completamente.
- **Salir completamente**: libera atajos globales, termina la alerta y el audio, detiene la red y quita el icono; cierra el proceso. También desde Configuración → Segundo plano → Salir de DameBola. Cerrar la ventana con el segundo plano desactivado (o sin bandeja) sale.
- **Atajo global opcional** con `tauri-plugin-global-shortcut`, reutilizando combinación, activación y destinatario existentes. Registro transaccional, validación de combinaciones reservadas por sistema, error visible si está ocupado, sin doble envío, repetición de tecla descartada.
- **Envío desde Rust** (`send.rs`): el atajo local, el global y la bandeja comparten destinatario, emparejamiento, conexión, cerrojo de envío simultáneo y `NetworkService::send_alert`. El frontend ya no resuelve destinatarios.
- **Inicio automático**: opción «Iniciar DameBola con el sistema» (reutiliza `tauri-plugin-autostart` de la Fase 1) y «Iniciar minimizado» (solo al iniciar con el sistema, con bandeja). La entrada de inicio ahora pasa `--autostart`; si ya estaba activada se reescribe al arrancar.
- **Configuración → Segundo plano** y **Alcance del atajo** (con advertencias). Aviso discreto del primer cierre (un diálogo nativo informativo, una sola vez; la marca la escribe el backend).
- **Compatibilidad**: campos nuevos con valores por defecto en el JSON de ajustes; instalaciones anteriores quedan con atajo **local**, conservan atajo, destinatario, canciones, identidad, certificados, emparejamientos y alias (no se tocó `identity`, `tls`, `store` ni el protocolo).

## Diferencias macOS / Windows

| | macOS | Windows |
|---|---|---|
| Icono | Barra de menús; clic abre el menú. | Bandeja; clic izquierdo abre la ventana, derecho el menú. |
| Cierre | Cierra → oculta; clic en el Dock reabre; Cmd+Q sale. | Cierra → oculta; Salir desde la bandeja. |
| Atajo global reservado | `Cmd+Q/H/M/W/F`, capturas de pantalla, etc. | Todo `Win + …`. |
| Combinación ocupada | El sistema puede permitir duplicados sin error. | `RegisterHotKey` falla y se informa. |
| Inicio con el sistema | LaunchAgent del usuario (Elementos de inicio). | Clave del usuario en el registro (sin administrador). |

## Cómo salir completamente

Menú de la bandeja → **Salir completamente**; o Configuración → Segundo plano → **Salir de DameBola**; o desactivar el segundo plano y cerrar la ventana; en macOS también Cmd+Q. Nunca se oculta el proceso: aparece en el Administrador de tareas / Monitor de Actividad.

## Limitaciones conocidas

- Sin notificaciones nativas: un envío desde atajo global/bandeja con la ventana oculta no muestra resultado local (con la ventana visible sí, como aviso). El destinatario ve la alerta.
- macOS no siempre informa «combinación ocupada» (observado: dos instancias registraron la misma combinación sin error).
- «Iniciar minimizado» solo afecta al inicio con el sistema, no a un inicio manual.
- Linux no es un objetivo (la bandeja depende del entorno de escritorio; si no se puede crear el icono, cerrar la ventana sale).
- El instalador de prueba de Windows no está firmado (SmartScreen avisará).

## Pruebas realizadas

Automáticas (ver resultados en el PR y la CI de macOS y Windows):

- Rust (`cargo test`): reglas de combinaciones globales por sistema y compatibilidad de formato con el plugin; registro transaccional (atajo ocupado conserva el anterior, cambio de atajo, volver a local libera, reservadas nunca se registran, liberar al salir); repetición de tecla y liberación perdida; destinatario (único, elegido, no emparejado, varios); envío solo a emparejado y conectado; envíos simultáneos rechazados y cerrojo liberado; ajustes (valores por defecto en instalaciones anteriores, persistencia, bandeja obligatoria para segundo plano, marca de aviso propiedad del backend); decisión de cierre (ocultar vs. salir), no terminar con la última ventana, inicio oculto; menú de bandeja (habilitación de «Enviar», mensajes, silenciar/reactivar).
- Frontend (`npm test`): el atajo local solo envía con alcance local, no hay doble envío al pasar a global, cambio local↔global en la siguiente pulsación, repetición, captura, avisos de combinaciones reservadas.
- Humo en macOS (compilación debug con la interfaz embebida, carpeta de datos aislada): la app arranca con el atajo global guardado y lo registra; cerrar la ventana con el botón real deja 0 ventanas y el proceso vivo; aparece el elemento de la barra de menús.

No automatizado (la CI no tiene pantalla, audio ni teclado): restaurar la ventana desde la bandeja, el menú de la bandeja con clics reales, Salir completamente, el envío real desde el atajo global/bandeja, inicio automático real tras reiniciar sesión. Las piezas de decisión de cada uno están probadas con pruebas unitarias; el cableado no.

## Pruebas manuales pendientes (equipos físicos macOS ↔ Windows)

1. Cerrar la ventana en cada equipo: aparece el aviso una sola vez; el otro equipo sigue viéndolo conectado; recibir una alerta con la ventana oculta (no se abre la principal; multimonitor y música).
2. Bandeja: Abrir (desde oculta y desde minimizada), Enviar (con y sin destinatario), Silenciar/Reactivar (el otro equipo recibe `muted`), casilla de inicio, Salir completamente (el otro equipo ve la desconexión).
3. Atajo global: ver la lista en `docs/KEYBOARD_SHORTCUT.md` (ventana visible, minimizada, oculta y otra app enfocada; tecla mantenida; combinación ocupada; volver a local; tras salir no funciona).
4. Iniciar con el sistema + Iniciar minimizado: cerrar sesión, volver a entrar; sin instancias duplicadas.
5. Actualizar desde 0.1.3 sin perder canciones, identidad, emparejamientos ni atajo.

## Cómo probarlo

- macOS: `npm ci && npm run tauri build -- --debug --no-bundle`, y ejecutar con `DAMEBOLA_DATA_DIR=/ruta/propia DAMEBOLA_KEY_STORAGE=file` para **no** compartir datos con la app instalada.
- Windows: instalador de prueba 0.1.4 del release (ver el PR). Desinstalar o cerrar antes la versión anterior.
