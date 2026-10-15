# DameBola 🐔

> Una aplicación de escritorio multiplataforma para llamar la atención de tus compañeros con alertas, música y humor.

DameBola nació para dos compañeros de oficina (uno con macOS y otro con Windows). Una alerta cubre todos los monitores con fondo rojo, un pollo bailando, el texto **¡¡¡DAME BOLA!!!** y una canción al azar de una biblioteca local.

[![CI](https://github.com/joacocap11/DameBola/actions/workflows/ci.yml/badge.svg)](https://github.com/joacocap11/DameBola/actions/workflows/ci.yml)
[![Licencia: MIT](https://img.shields.io/badge/licencia-MIT-red.svg)](LICENSE)

> ⚠️ **Estado actual: Fase 2 en revisión (rama `feature/phase2-p2p`).** La comunicación entre computadoras de la misma LAN está implementada, pero **todavía no fue probada entre dos equipos físicos ni en Windows**. En `main` (Fase 1) la comunicación entre computadoras **no existe**. Ver [Estado](#estado-actual) y [Roadmap](#roadmap).

## Capturas

Todavía no hay capturas publicadas.

## Características

- Alerta local multimonitor: una ventana Tauri por pantalla, siempre al frente, con pollo animado propio (SVG + CSS), cierre con botón, **Esc** o automático (15 s por defecto).
- Audio reproducido **una sola vez**, sin importar cuántos monitores haya; canción aleatoria entre las activas, sin repetir la anterior si hay al menos dos.
- ABM de canciones (MP3, WAV y OGG): copia real al directorio de datos, validación de contenido, detección de duplicados por SHA-256, búsqueda, filtros, orden, renombrar, activar/desactivar y eliminar con confirmación.
- Reproducción de prueba desde el listado (reproducir, pausar, detener).
- Configuración persistente: nombre, volumen (0–100), duración, sonido, animaciones e inicio con el sistema.
- Interfaz en español, tema oscuro y rojo.
- Seguridad: el frontend no tiene acceso al sistema de archivos ni abre sockets; todo se hace en Rust con ids internos.
- **Fase 2 (P2P en LAN, sin servidores ni Internet):** descubrimiento mDNS y conexión manual por IP y puerto, emparejamiento con código de verificación de 8 dígitos, identidad criptográfica por dispositivo, canal TLS 1.3 autenticado y cifrado, envío de alertas en ambos sentidos con confirmación real (`mostrada`), antispam (1 alerta cada 15 s por remitente), silencio temporal, desactivar alertas remotas o toda la red, heartbeats y reconexión automática. Ver [docs/NETWORK_PROTOCOL.md](docs/NETWORK_PROTOCOL.md) y [docs/PAIRING_SECURITY.md](docs/PAIRING_SECURITY.md).

## Tecnologías

Tauri 2 · React 19 · TypeScript (estricto) · Vite · Tailwind CSS 4 · Rust · SQLite (`rusqlite`, embebido) · `rodio` (audio) · `tokio` + `tokio-tungstenite` (WebSocket) · `rustls` (TLS 1.3) · `rcgen` · `mdns-sd` · `keyring`.

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

## Estabilización (Fase 2.5)

La rama `feature/phase2-stability` corrige: la identidad ya no se regenera al cambiar de almacén de claves (error recuperable + recuperación explícita), el descubrimiento rechaza direcciones públicas y de otras subredes, *Desconectar* se respeta (también tras reiniciar), «se mostró» solo se confirma si **todas** las ventanas se abrieron, y mDNS no pisa nombres guardados (alias local). Detalles: [PHASE2_STABILITY_STATUS.md](PHASE2_STABILITY_STATUS.md). El protocolo de red no cambió.

Para ejecutar una instancia de **desarrollo** sin riesgo para tu instalación real, usá una carpeta propia: `DAMEBOLA_DATA_DIR="$(mktemp -d)" npm run tauri dev`.

## Segundo plano y atajos globales (Fase 2.6)

- **Cerrar la ventana no cierra DameBola**: se oculta y la red, la recepción de alertas y los atajos siguen activos. La primera vez se muestra un aviso. Se desactiva en Configuración → Segundo plano; entonces cerrar la ventana sale.
- **Bandeja del sistema (Windows) / barra de menús (macOS)**: Abrir DameBola, Enviar DAME BOLA, Silenciar/Reactivar alertas, Iniciar con el sistema y **Salir completamente**. Muestra el estado de conexión y el destinatario; «Enviar» se deshabilita si no hay un destinatario válido y conectado.
- **Atajo global (opcional)**: Configuración → Alcance del atajo → Global, p. ej. `Ctrl + Shift + B` / `Cmd + Shift + B`. Por defecto el atajo sigue siendo local (solo con la ventana enfocada). Ver [docs/KEYBOARD_SHORTCUT.md](docs/KEYBOARD_SHORTCUT.md).
- **Iniciar con el sistema** e **Iniciar minimizado**: solo si los activás; sin permisos de administrador.
- Estado, diferencias entre sistemas, limitaciones y pruebas pendientes: [PHASE2_BACKGROUND_STATUS.md](PHASE2_BACKGROUND_STATUS.md).

## Red entre equipos (Fase 2)

1. Abrí **Dispositivos** en ambos equipos (misma red). Aparecen bajo «Descubiertos»; si mDNS está bloqueado, **Agregar dispositivo manualmente** (IP y puerto).
2. **Emparejar** en uno: ambos muestran el mismo código de 8 dígitos. Comparenlo y confirmen en los dos.
3. En el dispositivo emparejado, **DAME BOLA**. La interfaz solo dice «se mostró» cuando el otro equipo confirmó que la alerta ya está en pantalla.

Puerto: **TCP 38473** (configurable), entrante. mDNS: UDP 5353 multicast (solo descubrimiento). No se toca el firewall ni el router y no se usa Internet. Guía de firewall y diagnóstico: [docs/NETWORK_TROUBLESHOOTING.md](docs/NETWORK_TROUBLESHOOTING.md).

## Datos de la aplicación

| | Ubicación |
|---|---|
| macOS | `~/Library/Application Support/com.damebola.app/` |
| Windows | `%APPDATA%\com.damebola.app\` |

Contiene `damebola.db` (metadatos, ajustes, certificado público y dispositivos emparejados) y `audio/` (copias de las canciones). La **clave privada** del dispositivo va en el llavero del sistema (o, si no está disponible, en `device.key` con permisos restringidos). Nada de esto se incluye en el repositorio.

## Estado actual

- Probado manualmente **solo en macOS** (Apple Silicon, 3 monitores): ABM, reproducción, alertas multimonitor, ajustes.
- **Fase 2:** pruebas automáticas con instancias reales en un mismo proceso (emparejamiento, alertas en ambos sentidos, revocación, antispam, protocolo inválido, reconexión) y una prueba manual de la app real contra un proceso par en la misma Mac, incluyendo mDNS. **No** se probó entre dos computadoras físicas.
- **Windows no fue probado manualmente.** La CI comprueba que compila y que pasan las pruebas automáticas, nada más.
- El inicio con el sistema está implementado pero no verificado.
- Detalle completo en [PHASE1_STATUS.md](PHASE1_STATUS.md).

## Limitaciones conocidas

- No hay sincronización de canciones (Fase 3). La comunicación entre equipos (Fase 2) solo funciona en la misma LAN, IPv4.
- Entre equipos: si no se comparan los códigos al emparejar, un intermediario no se detecta (ver [docs/PAIRING_SECURITY.md](docs/PAIRING_SECURITY.md)).
- macOS: el Dock y la barra de menú pueden quedar visibles sobre la alerta; una app en pantalla completa nativa puede quedar por encima.
- Windows: la barra de tareas puede quedar por encima si el usuario la activa; el posicionamiento con monitores de DPI distinto no está verificado.
- Escape requiere que la ventana de alerta tenga el foco; los botones funcionan siempre.
- Formatos: MP3, WAV y OGG (Vorbis), hasta 100 MB por archivo.

## Roadmap

1. **Fase 1** ✅ Aplicación local: ABM de canciones, alertas multimonitor, configuración.
2. **Fase 2** 🔎 Comunicación P2P en LAN y sección «Dispositivos»: implementada en la rama `feature/phase2-p2p` (PR abierto); falta probarla entre dos equipos físicos (macOS y Windows).
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
