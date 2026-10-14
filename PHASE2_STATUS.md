# Estado de la Fase 2 (comunicación P2P en LAN)

Rama: `feature/phase2-p2p` (Pull Request hacia `main`, **sin fusionar** hasta tener pruebas manuales entre equipos).

## Implementado

- **Red P2P en Rust** (`src-tauri/src/net/`): cada instalación es servidor y cliente WebSocket sobre TLS 1.3 con autenticación mutua. El frontend no abre sockets.
- **Identidad criptográfica** persistente por dispositivo (ECDSA P-256, certificado autofirmado), `deviceId` derivado del fingerprint; clave privada en el llavero del SO con respaldo en archivo `0600`.
- **Emparejamiento seguro:** solicitud explícita, aceptación explícita de ambos lados y código de verificación de 8 dígitos derivado del canal TLS (resiste intermediarios). Credencial permanente = fingerprint fijado. Revocación (`UNPAIR`).
- **Descubrimiento** mDNS (`_damebola._tcp.local.`) solo por interfaces LAN; no se muestra el propio equipo; entradas con expiración. **Conexión manual** por IP y puerto con las mismas garantías (solo direcciones de red local).
- **Pantalla «Dispositivos»:** este equipo (nombre, estado, IP, puerto, huella, almacenamiento de clave), ajustes de red, descubiertos y emparejados con acciones Emparejar / Conectar / Desconectar / DAME BOLA / Olvidar, agregar manualmente, diálogo de emparejamiento.
- **Alertas remotas** que reutilizan `alert::start` de la Fase 1; resultados distinguidos (`enviada → aceptada → mostrada` / `rechazada` / `sin conexión` / `sin confirmación`); solo «mostrada» se presenta como entregada.
- **Antispam y control del destinatario:** 1 alerta cada 15 s por remitente (configurable 5–3600 s), sin cola, silencio temporal, desactivar alertas remotas, desactivar toda la red, deduplicación de mensajes.
- **Estado y reconexión:** latidos, detección de desconexión, reconexión con backoff exponencial y jitter, desconexión manual que no reconecta sola, detección de suspensión/reanudación y de cambios de IP.
- **Persistencia:** migración 2 (`device_identity`, `paired_devices`) compatible con bases de la Fase 1; ajustes de red en `settings`.
- Protocolo versionado con mensajes estrictos y límites de tamaño/frecuencia.

## Pruebas

Automáticas (`npm run typecheck`, `npm test`, `npm run test:rust`, `npm run build`, `cargo check`):

- Frontend: 18 pruebas (Fase 1 + validación de IP/puerto, tiempos relativos, silencio).
- Rust: 83 pruebas (+1 ignorada de mDNS que requiere LAN real). Nuevas de la Fase 2:
  - Protocolo: ida y vuelta de los 11 tipos, formato en el cable, tamaño máximo, versión, claves/tipos/campos desconocidos, validación de `HELLO`, saneado de nombres.
  - Identidad: derivación del id, persistencia, regeneración si la clave no coincide, clave fuera de SQLite, permisos `0600`.
  - TLS: autenticación mutua, pinning (un impostor falla), **intermediario ⇒ códigos distintos**, códigos distintos por sesión.
  - Antispam: rate limit por remitente, token bucket, deduplicación acotada y con TTL.
  - Migraciones: una base de Fase 1 con canciones y ajustes conserva todo tras migrar; idempotencia; restricciones.
  - **Integración con dos/tres instancias reales** (puertos distintos, 127.0.0.1): emparejamiento completo con códigos iguales; rechazo del receptor; «no coincide» del iniciador; expiración; **A→B y B→A**; no emparejado no puede enviar; abuso ⇒ conexión cerrada; revocación en ambos lados; receptor que borró el emparejamiento ⇒ `unauthorized`; **duplicados se procesan una vez**; rate limit; alertas desactivadas/silenciadas/ocupado/falla ≠ «mostrada»; destinatario desconectado ⇒ `offline`; reconexión automática; desconexión manual sin reconexión; certificado distinto ⇒ handshake falla; entradas manuales inválidas (incluye IP pública y el propio equipo); desactivar la red cierra el puerto; basura, mensajes gigantes, mensajes antes de `HELLO`, `deviceId` ajeno, tipos desconocidos y frames binarios cierran la conexión.
- **CI en Windows y macOS** (runners oficiales): ejecutan todas las pruebas, incluida la integración en loopback con TLS+WebSocket reales. En Windows aparecieron dos problemas que macOS no mostró (un `PAIR_REJECT` perdido por cerrar el socket con datos sin leer, y el puerto que tarda unos ms en liberarse al reactivar la red); se corrigieron (cierre ordenado y reintento de `bind`) y la CI quedó en verde. Esto **no** equivale a probar dos equipos Windows/Mac reales.
- Prueba ignorada `mdns_discovers_the_other_instance_and_expires_it_when_it_leaves`: pasó en esta Mac con una interfaz LAN real (`cargo test -- --ignored mdns`). No corre en CI (los runners no garantizan multicast).
- Comprobado una vez a mano que `KeyringStore` guarda, lee y borra en el **Keychain de macOS** (prueba descartable).

Manuales en macOS con la app real (`npm run tauri dev`) contra un **proceso par independiente en la misma Mac** (otra identidad, otro puerto, IP de LAN real):
- La app descubre al par por **mDNS** y lo muestra en «Descubiertos».
- Emparejar desde la interfaz: ambos muestran el **mismo código**; al confirmar queda emparejado en ambos.
- Alerta par → app: se muestra en los 3 monitores con el nombre del remitente y una canción de la biblioteca; la interfaz del par recibió `shown`.
- Alerta app → par desde el botón DAME BOLA: la interfaz mostró «La alerta se mostró en …» y el par la recibió.
- Tras reiniciar la app, el emparejamiento persistió y se **reconectó solo**.
- Para evitar avisos del llavero en desarrollo se usó `DAMEBOLA_KEY_STORAGE=file`; los datos de prueba se borraron después.

## Pendiente / no verificado

- **Dos equipos físicos (Mac ↔ Windows): no probado.** Windows solo se ejercitó en el runner de CI (compilación + pruebas automáticas); no hubo prueba manual con la interfaz, el firewall ni audio/ventanas.
- Llavero real dentro de la app (macOS) y Windows Credential Manager: la ruta está implementada; en la prueba manual se usó el archivo de respaldo.
- Suspensión/reanudación y cambio de IP reales: la lógica está implementada (salto de reloj, sondeo de interfaces cada 5 s) pero no se forzó en una máquina.
- Reglas de firewall de Windows y permiso de «Red local» de macOS: documentados, no verificados en equipos reales.
- Sin IPv6. Sin rotación de claves. Sin sincronización de canciones (Fase 3).
- La CI no puede probar mDNS, audio ni ventanas reales.

## Limitaciones conocidas

- Si los usuarios no comparan el código, un intermediario durante el primer emparejamiento no se detecta (ver [docs/PAIRING_SECURITY.md](docs/PAIRING_SECURITY.md)).
- Reinstalar o regenerar la identidad obliga a emparejar de nuevo.
- «Olvidar» avisa al otro equipo solo si hay conexión; si no, el otro conserva su registro (pero será rechazado).
- Los nombres y el SO previos al emparejamiento no están autenticados.
- Los rangos privados usados por VPN cuentan como «LAN».

## Pasos para verificar entre una Mac y una PC

1. Rama `feature/phase2-p2p` en ambos: `npm install && npm run tauri dev` (o `npm run tauri build` y ejecutar el instalador).
2. Permitir el firewall/Red local al primer aviso (ver [docs/NETWORK_TROUBLESHOOTING.md](docs/NETWORK_TROUBLESHOOTING.md)); en Windows, red **Privada**.
3. Comprobar el puerto: `nc -vz <IP> 38473` (Mac) / `Test-NetConnection <IP> -Port 38473` (PC).
4. **Dispositivos**: ¿se ven mutuamente? Si no, **Agregar dispositivo manualmente**.
5. **Emparejar** desde uno; comparar el código; confirmar en ambos.
6. **DAME BOLA** en ambos sentidos: alerta en todos los monitores del destinatario y un solo audio; el emisor ve «se mostró».
7. Probar: segunda alerta antes de 15 s (rechazada), «Silenciar», desactivar alertas remotas, desconectar la red/cable (estado desconectado y reconexión), cerrar una app (estado), **Olvidar** y comprobar que ya no se pueden enviar alertas.
8. Registrar resultados y problemas en el PR.
