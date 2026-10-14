# Estado de la Fase 1

Verificado en: macOS (Apple Silicon), 3 monitores, Tauri 2.12, Rust 1.98, Node 24.
**Windows no fue probado** (no hay equipo disponible en este entorno).

## Funcionalidades implementadas

- Navegación lateral (Inicio, Canciones, Configuración), tema oscuro rojo, interfaz en español.
- Inicio: nombre, pollo animado, «Modo local», contador de canciones disponibles, botón **PROBAR DAME BOLA**.
- Alerta local: una ventana Tauri por monitor, fondo rojo, texto, pollo (SVG+CSS), botón **YA TE DOY BOLA**, Escape, cierre automático (duración configurable, 15 s por defecto), destrucción de ventanas al terminar, una sola alerta a la vez, audio una única vez, alerta sin audio si no hay canciones.
- ABM de canciones: alta múltiple (MP3/WAV/OGG) con copia real, validación de extensión + firma + decodificación, SHA-256 anti-duplicados; listado con búsqueda, filtro por estado, orden por nombre/fecha, contador; renombrar, activar/desactivar; eliminar con confirmación (borra registro y archivo); estados de carga/vacío/error; avisos de éxito/error.
- Reproducción desde el listado: reproducir, pausar, reanudar, detener, una sola pista simultánea.
- Selección aleatoria: solo activas con archivo existente; no repite la anterior si hay ≥ 2.
- Configuración persistente: nombre, volumen 0–100, duración 3–120 s, sonido, animaciones, inicio con el sistema.
- SQLite en el directorio de datos del SO; audio en `audio/`; ids UUID y hash único listos para sincronizar.

## Funcionalidades probadas

Automatizadas (`npm run test:rust`: 25 pruebas; `npm test`: 7 pruebas; `npm run typecheck`; `npm run build`; `cargo check`):
- Selección aleatoria (vacío, uno, exclusión de la última, rango).
- ABM sobre SQLite y archivos reales en directorios temporales: importar/copiar, independencia del original, duplicados por contenido, rechazo de extensión inválida/archivo vacío/falso/cabecera sin datos/extensión que no coincide con el contenido, lote mixto, renombrar/activar con validación, eliminar (registro + archivo), ids maliciosos (`../x`, rutas absolutas) rechazados sin tocar archivos, archivo ausente, persistencia tras reabrir la base.
- Ajustes: valores por defecto, persistencia, rangos.
- Filtro/orden/búsqueda del frontend.

Manuales en macOS con la app real (`npm run tauri dev`, con clics reales y capturas):
- Importar 5 archivos con el selector nativo: 3 importados (MP3, WAV, OGG), 1 duplicado detectado, 1 falso rechazado, con sus mensajes.
- Alerta con y sin canciones: 3 ventanas (3 monitores), cobertura completa, nombre de la canción mostrado, cierre automático a los 15 s, cierre con Escape, retorno a la ventana principal.
- Reproducción desde el listado: el estado pasa a «reproduciendo» y vuelve a «detenido» al terminar la pista.
- Eliminar con confirmación: archivo y fila desaparecen.
- Guardar ajustes: persistidos en SQLite.
- Bug hallado y corregido: `rodio` pedía un buffer de 2048 muestras y el dispositivo de salida lo rechazaba (rango 14..=1024); ahora se abre con el buffer por defecto del sistema.

## Pendiente / sin verificar

- **Windows completo**: compilación, posicionamiento multimonitor con DPI mixto, comportamiento de la barra de tareas, instalador.
- **Inicio con el sistema**: implementado con `tauri-plugin-autostart` (LaunchAgent / registro), pero no activé la opción para no dejar el binario de desarrollo registrado. Probar con una build instalada.
- No pude *escuchar* el audio (el entorno de pruebas no lo permite); verifiqué que el stream abre, la pista se consume y el estado se notifica.
- Edición (modal) de canciones: cubierta por pruebas de backend; no recorrida con clics.
- No se generó instalador (`tauri build`).
- Fuera de alcance (fases siguientes): comunicación entre equipos, sincronización de canciones, sección Dispositivos.

## Limitaciones conocidas

- macOS: Dock y barra de menú pueden quedar visibles sobre la alerta; una app en pantalla completa nativa puede quedar por encima.
- Windows: la barra de tareas puede quedar por encima si el usuario la activa.
- El foco para Escape depende de que el SO entregue el foco a la ventana (en alertas remotas futuras, con la app en segundo plano, puede requerir un clic; los botones siempre funcionan).
- Los formatos se limitan a MP3, WAV y OGG (Vorbis); tamaño máximo 100 MB por archivo.
- El volumen configurado también aplica a las pruebas desde el listado.
- Si se borra un archivo de `audio/` por fuera de la app, la canción aparece como «Archivo faltante» y se excluye del sorteo.
- Los avisos de importación son toasts temporales.

## Pasos para verificar manualmente

1. `npm install && npm run tauri dev`.
2. Inicio: sin canciones, pulsar **PROBAR DAME BOLA** → alerta sin audio; cerrar con Esc.
3. Canciones → **Agregar canciones**: elegir varios MP3/WAV/OGG, repetir con un archivo duplicado y con un `.mp3` que sea un texto renombrado → mensajes de duplicado/error.
4. Reproducir/pausar/detener una canción; iniciar otra y comprobar que la primera se detiene.
5. Editar nombre y desactivar; cerrar y abrir la app → persiste.
6. **PROBAR DAME BOLA**: ventanas en todos los monitores, un único audio, cierre con botón/Esc/15 s; durante la alerta, el botón de inicio queda deshabilitado.
7. Desactivar todas las canciones salvo una: dos alertas seguidas repiten esa canción; con dos activas nunca se repite seguida.
8. Eliminar una canción → confirmar → desaparece de la lista y de `audio/`.
9. Configuración: cambiar volumen/duración/animaciones/sonido, guardar, reiniciar.
10. Windows: repetir 1–9, especialmente multimonitor con distinta escala.
