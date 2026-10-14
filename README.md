# DameBola 🐔

> Una aplicación de escritorio multiplataforma para llamar la atención de tus compañeros con alertas, música y humor.

DameBola nació para dos compañeros de oficina (uno con macOS y otro con Windows). Una alerta cubre todos los monitores con fondo rojo, un pollo bailando, el texto **¡¡¡DAME BOLA!!!** y una canción al azar de una biblioteca local.

[![CI](https://github.com/joacocap11/DameBola/actions/workflows/ci.yml/badge.svg)](https://github.com/joacocap11/DameBola/actions/workflows/ci.yml)
[![Licencia: MIT](https://img.shields.io/badge/licencia-MIT-red.svg)](LICENSE)

> ⚠️ **Estado actual: Fase 1 (modo local).** La **comunicación entre computadoras todavía no está implementada**: hoy la alerta solo se puede probar en la misma máquina. Ver [Estado](#estado-actual) y [Roadmap](#roadmap).

## Capturas

Todavía no hay capturas publicadas.

## Características

- Alerta local multimonitor: una ventana Tauri por pantalla, siempre al frente, con pollo animado propio (SVG + CSS), cierre con botón, **Esc** o automático (15 s por defecto).
- Audio reproducido **una sola vez**, sin importar cuántos monitores haya; canción aleatoria entre las activas, sin repetir la anterior si hay al menos dos.
- ABM de canciones (MP3, WAV y OGG): copia real al directorio de datos, validación de contenido, detección de duplicados por SHA-256, búsqueda, filtros, orden, renombrar, activar/desactivar y eliminar con confirmación.
- Reproducción de prueba desde el listado (reproducir, pausar, detener).
- Configuración persistente: nombre, volumen (0–100), duración, sonido, animaciones e inicio con el sistema.
- Interfaz en español, tema oscuro y rojo.
- Seguridad: el frontend no tiene acceso al sistema de archivos; todo se hace en Rust con ids internos.

## Tecnologías

Tauri 2 · React 19 · TypeScript (estricto) · Vite · Tailwind CSS 4 · Rust · SQLite (`rusqlite`, embebido) · `rodio` (audio).

## Requisitos

| | macOS | Windows |
|---|---|---|
| Node.js 20 o superior | ✔ | ✔ |
| Rust estable 1.87 o superior ([rustup](https://rustup.rs)) | ✔ | ✔ (toolchain MSVC) |
| Compilador | Xcode Command Line Tools: `xcode-select --install` | Visual Studio Build Tools con «Desktop development with C++» |
| WebView | WKWebView (incluido) | WebView2 (incluido en Windows 10/11 actuales) |

No hace falta instalar SQLite: se compila dentro de la app.

## Instalación y ejecución en desarrollo

```bash
git clone https://github.com/joacocap11/DameBola.git
cd DameBola
npm install
npm run tauri dev
```

## Compilación

```bash
npm run build          # solo el frontend (typecheck + Vite)
npm run tauri build    # instalador: .app/.dmg en macOS, .msi/.exe en Windows
```

Los instaladores se generan en el sistema operativo de destino. Los binarios resultantes no están firmados.

## Pruebas

```bash
npm run typecheck      # TypeScript
npm test               # frontend (vitest)
npm run test:rust      # backend (cargo test)
```

La integración continua (GitHub Actions) ejecuta typecheck, pruebas, build del frontend y compilación de la app Tauri en macOS y Windows. **No** puede probar audio real ni ventanas multimonitor (los runners no tienen dispositivo de audio ni pantallas reales); eso se verifica a mano.

## Datos de la aplicación

| | Ubicación |
|---|---|
| macOS | `~/Library/Application Support/com.damebola.app/` |
| Windows | `%APPDATA%\com.damebola.app\` |

Contiene `damebola.db` (metadatos y ajustes) y `audio/` (copias de las canciones). Las canciones importadas nunca se incluyen en el repositorio.

## Estado actual

- Probado manualmente **solo en macOS** (Apple Silicon, 3 monitores): ABM, reproducción, alertas multimonitor, ajustes.
- **Windows no fue probado manualmente.** La CI comprueba que compila y que pasan las pruebas automáticas, nada más.
- El inicio con el sistema está implementado pero no verificado.
- Detalle completo en [PHASE1_STATUS.md](PHASE1_STATUS.md).

## Limitaciones conocidas

- Sin comunicación entre equipos ni sincronización de canciones (fases siguientes).
- macOS: el Dock y la barra de menú pueden quedar visibles sobre la alerta; una app en pantalla completa nativa puede quedar por encima.
- Windows: la barra de tareas puede quedar por encima si el usuario la activa; el posicionamiento con monitores de DPI distinto no está verificado.
- Escape requiere que la ventana de alerta tenga el foco; los botones funcionan siempre.
- Formatos: MP3, WAV y OGG (Vorbis), hasta 100 MB por archivo.

## Roadmap

1. **Fase 1** ✅ Aplicación local: ABM de canciones, alertas multimonitor, configuración.
2. **Fase 2** Comunicación entre dos computadoras (WebSocket) y sección «Dispositivos»: enviar una alerta al otro equipo.
3. **Fase 3** Sincronización de la biblioteca de canciones entre equipos.
4. Verificación y empaquetado en Windows; instaladores firmados.

## Reemplazar el pollo por un GIF

Ver `src/config/alertMedia.ts`. Usá únicamente imágenes propias o con licencia que permita redistribuirlas.

## Arquitectura

Ver [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## Contribuir y seguridad

[CONTRIBUTING.md](CONTRIBUTING.md) · [SECURITY.md](SECURITY.md)

## Licencia

[MIT](LICENSE)
