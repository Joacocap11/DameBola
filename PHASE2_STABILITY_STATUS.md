# Fase 2.5 — Estabilización P2P

Rama: `feature/phase2-stability` (PR hacia `feature/phase2-hotkey`; ambos PR anteriores siguen sin fusionar). Versión de prueba: **0.1.3**.

**El protocolo de red NO cambió** (sigue en la versión 1). Cambios de datos: migración 3 de SQLite (solo agrega columnas con valor por defecto y una tabla). El sistema de atajos no se tocó.

## Problemas corregidos

### 1. Identidad criptográfica (crítico)
- **Causa raíz.** `load_or_create` creaba un par nuevo siempre que faltaba el certificado *o* la clave, o no coincidían, y lo guardaba pisando el certificado (`set_identity_cert` con *upsert*). Además `open_secret_store` caía en silencio al archivo si el llavero no respondía, y `DAMEBOLA_KEY_STORAGE=file` se aceptaba sobre la carpeta de datos real. Cualquier cambio de almacén, llavero bloqueado o instancia de desarrollo mal aislada regeneraba la identidad.
- **Cambios.**
  - Con certificado existente se busca la clave en **todos** los almacenes (llavero y archivo; ranura propia y ranura histórica), solo lectura y sin copiar entre ellos. Si no hay una clave que forme par válido con el certificado (comprobación criptográfica + aceptación de rustls) → error tipado (`key_missing`, `key_mismatch`, `store_unavailable`, `cert_missing`, `cert_corrupt`), la **red no arranca**, no se abre ningún puerto y no se modifica nada. El resto de la app sigue funcionando.
  - Sin certificado pero con emparejamientos o una identidad archivada → `cert_missing`: no se crea otra. Solo una instalación sin evidencia previa crea la primera.
  - El certificado ya no se sobrescribe (`create_identity_cert` solo inserta).
  - Recuperación explícita (`net_reset_identity`, confirmación escrita `NUEVA IDENTIDAD`, solo con un problema no temporal): clave nueva en **ranura propia** (nunca pisa una clave existente), certificado anterior archivado en `identity_archive`, emparejamientos locales conservados. Un llavero inaccesible es *temporal*: solo se ofrece **Reintentar**.
  - Device ID derivado del fingerprint → estable mientras no se reemplace la identidad. Los archivos de clave se crean con `create_new` (no se pisan) y permisos 0600; no se imprimen claves (prueba de `Debug`).
  - **Aislamiento:** `DAMEBOLA_KEY_STORAGE` y `DAMEBOLA_ALLOW_LOOPBACK` se **ignoran** sin `DAMEBOLA_DATA_DIR`; bloqueo exclusivo `instance.lock` impide dos procesos sobre la misma carpeta de datos.
- **Identidad real del usuario.** No se tocó ni se leyó (ni llavero ni base real). La nueva versión **verifica al arrancar** que clave y certificado forman par válido y lo muestra en Dispositivos; si no, informa sin modificar nada. Esa comprobación sobre la identidad real queda pendiente de ejecutar por el usuario con esta versión.

### 2. Direcciones IP públicas
- **Causa raíz.** `on_found` aceptaba cualquier dirección anunciada por mDNS (solo se filtraba IPv4), `connect_device` no revalidaba, y la conexión manual usaba una función propia más laxa.
- **Cambios.** Un único `AddrPolicy` (`net/ipfilter.rs`) para mDNS, manual y reconexión. Admite solo privadas y link-local (loopback únicamente con `allow_loopback`, para pruebas); rechaza públicas, CGNAT 100.64/10, no especificada, multicast, broadcast, documentación, benchmarking, reservadas, IPv6 link-local y la forma IPv4-mapeada de todas ellas. El **descubrimiento** exige además estar en la **misma subred** de alguna interfaz (con varias interfaces vale cualquiera; no red/broadcast/propia). Direcciones ya guardadas y manuales admiten otras subredes privadas (documentado). TLS con pinning y autenticación siguen obligatorios; no hay UPnP ni apertura de puertos.

### 3. Desconexión manual
- **Causa raíz.** *Desconectar* solo marcaba un conjunto en memoria para no marcar nosotros; el **otro equipo** veía un corte involuntario, redialaba y nuestra app aceptaba la conexión entrante. Tampoco sobrevivía a un reinicio.
- **Cambios.** `paired_devices.auto_connect` persiste la intención (reinicio incluido). Se rechaza toda conexión (entrante o saliente en curso) de un dispositivo desconectado a propósito al registrar su `HELLO` (cierra la carrera con intentos en curso). `reconnect_pass` y `connect_device` lo respetan (mDNS, cambio de IP y reinicio no lo reactivan; los latidos no conectan). El par recibe un cierre WebSocket `4001` y tampoco insiste (sin cambiar el protocolo). *Conectar* manual lo reactiva. Los cortes involuntarios siguen reconectando con backoff.

### 4. Confirmación de alertas
- **Causa raíz.** `create_windows` ignoraba el resultado de `show()` y no tenía política para fallos parciales.
- **Cambios.** `open_all` sobre `Surface`/`WindowHandle`: **todo o nada**. Si falla la creación, el posicionamiento o `show` de **cualquier** ventana, se destruyen las ya abiertas (sin huérfanas), no suena audio y se responde `ALERT_REJECT failed` (el emisor ya recibió `accepted`). «shown» = todas las ventanas creadas, posicionadas y mostradas sin error; **no** garantiza que alguien la viera. El foco es mejor esfuerzo. Un fallo no consume el turno de frecuencia; un éxito sí; sigue habiendo una sola alerta a la vez.

### 5. Nombres de dispositivos
- **Causa raíz.** `on_found` llamaba a `touch_paired` con el nombre del TXT de mDNS (no autenticado) y lo escribía en el registro guardado.
- **Cambios.** `name` = último nombre **anunciado y autenticado** (HELLO por TLS); `alias` = nombre local del usuario (`net_rename_device`), con prioridad y nunca pisado. mDNS solo alimenta la entrada de «descubiertos» en memoria (una por `device_id`, sin duplicados). El remitente que ve el receptor usa su alias. Migración conserva los nombres existentes.

## Pruebas

Automáticas (macOS, local): `npm run typecheck`, `npm test` (95), `npm run build`, `cargo check`, `cargo test` (**140** + 1 ignorada de mDNS). Nuevas, por problema:
- **Identidad:** llavero disponible; llavero inaccesible (temporal) y recuperación; cambio llavero↔archivo sin copiar ni regenerar; ranura histórica; clave ausente; clave incompatible (no se sobrescribe y se repara restaurándola); certificado dañado; emparejados sin certificado; reset con confirmación exacta, ranura nueva, archivo y conservación de emparejamientos; persistencia tras reiniciar (almacén en archivo en directorio temporal); archivo no sobrescribible y 0600; servicio: red detenida sin puerto, estado y reintento; aislamiento (`instance.rs`: variables ignoradas sin carpeta aislada, bloqueo exclusivo).
- **Red/IP:** clasificación IPv4/IPv6 (públicas, privadas, loopback, link-local, CGNAT, multicast, broadcast, documentación, mapeadas), múltiples interfaces, subred/broadcast/propia, `/31` y `/32`, origen manual/almacenado/descubrimiento; integración: descubrimiento descarta públicas/otra subred, direcciones guardadas públicas no se marcan, manual aplica la misma política y loopback exige opt-in.
- **Desconexión:** desconexión voluntaria no revertida por mDNS en ambos lados; reconexión manual; corte involuntario reconecta; reinicio (con y sin desconexión voluntaria); par que insiste es rechazado; carrera conectar/desconectar (8 iteraciones).
- **Alertas:** `open_all` con todos los monitores, fallo de la primera, de una secundaria, de `show` en cada ventana, sin monitores/errores, un monitor; sin huérfanas ni foco en alertas canceladas; turno único y liberación tras fallo; temporizador viejo; red: fallo de visualización ⇒ `failed`, nunca `shown`, y límite de frecuencia intacto.
- **Nombres:** anuncios repetidos/hostiles no pisan nada ni duplican; alias gana y persiste; cambio de nombre llega por HELLO autenticado; quitar alias; persistencia tras reiniciar; no emparejados muestran el último anunciado; remitente con alias.
- **Migración:** base sintética en versión 2 conserva nombres e identidad; idempotencia.

CI (GitHub Actions, macOS y Windows): ver el estado en el PR.

**No se ejecutaron pruebas manuales** en equipos físicos para esta fase ni se interactuó con la aplicación real del usuario. Interfaz nueva (tarjeta de identidad, alias, «Desconectado por vos»): cubierta por pruebas de lógica pura y compilación, **sin recorrido manual en la app real**.

## Compatibilidad
- Datos: canciones, ajustes, atajo (F12 por defecto), destinatario, emparejamientos y certificados existentes **se conservan**; la migración 3 solo agrega `alias`, `auto_connect` (por defecto 1) e `identity_archive`.
- Claves existentes (ranura histórica `device-private-key` / `device.key`) se siguen encontrando; no se mueven ni se reescriben.
- Protocolo v1 sin cambios; un par 0.1.2 interopera (ve un cierre normal en lugar de `4001`).

## Riesgos restantes
- La identidad **real** del usuario aún no se verificó con esta versión (ver arriba). Llavero de macOS y Credential Manager de Windows solo se ejercitan con almacenes simulados; el arranque real con llavero está pendiente de prueba manual.
- Un par 0.1.2 que sigue marcando tras un *Desconectar* es rechazado con backoff creciente (≤ 60 s): tráfico pequeño pero no nulo hasta que actualice.
- «shown» no demuestra que una persona la vio. Política todo-o-nada: un monitor con problemas impide la alerta completa (decisión documentada).
- Subredes privadas enrutadas: aceptadas solo por conexión manual/guardada (no hay descubrimiento entre subredes). CGNAT/Tailscale no se aceptan.
- El bloqueo de instancia no protege frente a versiones anteriores (sin bloqueo) ejecutándose sobre la misma carpeta.
- `remote_paused` es en memoria: tras reiniciar, un equipo que fue cortado a propósito reintenta una vez y se vuelve a pausar con el `4001`.

## Procedimientos de recuperación
Ver [docs/PAIRING_SECURITY.md → Recuperación de la identidad](docs/PAIRING_SECURITY.md#recuperación-de-la-identidad) y [docs/NETWORK_TROUBLESHOOTING.md](docs/NETWORK_TROUBLESHOOTING.md).

## Cómo probar manualmente
1. **Instancia aislada** (nunca sobre tus datos reales): `DAMEBOLA_DATA_DIR="$(mktemp -d)" npm run tauri dev` (cambiá el puerto en Dispositivos si otra instancia usa 38473). En Windows: `$env:DAMEBOLA_DATA_DIR = Join-Path $env:TEMP "damebola-test"`.
2. **Identidad:** en la instancia aislada con `DAMEBOLA_KEY_STORAGE=file`, empareja; cerrá, borrá **solo** `device-key-*.key` de esa carpeta temporal y reabrí: debe mostrar el error «No se encontró la clave…» sin crear otra identidad ni abrir puertos. *Reintentar* sigue fallando; restaurá el archivo y vuelve la misma identidad.
3. **IP:** conexión manual con `8.8.8.8` o `100.64.0.1` → rechazada con mensaje claro.
4. **Desconexión:** emparejá, **Desconectar** en uno: no vuelve a conectar (esperá >1 min, cambiá de red Wi-Fi, reiniciá la app); aparece «Desconectado por vos»; **Conectar** lo restablece. Cortá el cable/Wi-Fi sin pulsar nada: se recupera solo.
5. **Alertas:** enviar una alerta con varios monitores: «se mostró» solo si aparece en todos. (La falla de ventanas se verifica en las pruebas automáticas.)
6. **Nombres:** renombrá el otro equipo con el lápiz (alias); cambiá su nombre de usuario en Configuración y reconectá: el alias se mantiene y «Anunciado como…» se actualiza.
