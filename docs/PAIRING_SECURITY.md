# Seguridad: identidad y emparejamiento

## Objetivo y modelo de amenaza

Dos personas, dos equipos, una **LAN no confiable** (otros equipos de la oficina pueden escanear puertos, observar o alterar el tráfico, y suplantar mDNS). Se busca que:

1. Solo un equipo **emparejado** pueda disparar una alerta.
2. Un atacante en la LAN no pueda hacerse pasar por un equipo emparejado.
3. Un atacante activo (intermediario) no pueda colarse durante el emparejamiento sin que los usuarios lo noten.
4. El destinatario conserve el control (desactivar, silenciar, límite de frecuencia, cerrar la alerta).

Fuera de alcance: un equipo ya comprometido (acceso a la clave privada o a la base local), y ataques de denegación de servicio volumétricos.

## Decisiones

| Tema | Decisión |
|---|---|
| Cifrado y autenticación del canal | **TLS 1.3** (`rustls` + proveedor `ring`), solo TLS 1.3. No hay criptografía propia. |
| Identidad | Par de claves **ECDSA P-256** y certificado autofirmado (`rcgen`), generado en el primer arranque y persistente. |
| ID público | `deviceId` = primeros 16 bytes (32 hex) del SHA-256 del certificado. Reclamar el id de otro equipo exige su clave privada. El `HELLO` se rechaza si su `deviceId` no coincide con el certificado TLS. |
| Autorización | Por **fingerprint del certificado** (pinning) contra `paired_devices`, en cada mensaje. |
| Conexiones a equipos emparejados | El cliente exige exactamente el certificado fijado. Un impostor falla el handshake TLS. |
| Almacenamiento de la clave privada | **Llavero del sistema** (macOS Keychain / Windows Credential Manager) mediante la crate `keyring`, o un archivo con permisos `0600` (en Windows hereda la ACL del perfil; protección más débil, y la pantalla Dispositivos lo indica). Se consultan **ambos** al cargar (solo lectura, sin copiar la clave de uno a otro). Nunca en SQLite, nunca por la red, nunca en el repositorio, nunca en logs. |
| Persistencia de la identidad (Fase 2.5) | Una identidad existente **nunca se regenera en silencio**: si falta la clave, no corresponde al certificado o el llavero no responde, la red no arranca y se muestra un error recuperable. Las claves nuevas usan una ranura propia (`key-<fingerprint>`) y no se sobrescriben nunca; el certificado anterior se archiva, no se borra. |
| Persistencia | SQLite guarda el certificado público, los emparejados (id, nombre, SO, fingerprint, última dirección) y los ajustes. |

### Por qué los verificadores TLS personalizados no «desactivan» la verificación

Con certificados autofirmados no hay CA. Los verificadores de `service`/`tls.rs`:

- **siempre** validan la firma del handshake TLS 1.3 con las rutinas de rustls (prueba de que el otro extremo posee la clave del certificado);
- rechazan certificados enormes y cadenas con intermedios;
- como cliente hacia un equipo emparejado exigen **igualdad exacta del fingerprint** (`PeerPin::Expect`);
- solo ante un equipo **desconocido** (descubrimiento/emparejamiento) aceptan cualquier certificado válido (`PeerPin::AcceptAny`). Esa sesión **no tiene autoridad**: solo puede hablar `HELLO`, `PAIR_*` y `PING`; el emparejamiento exige verificar el código.

Como servidor, se pide certificado de cliente y se acepta cualquiera bien formado que pruebe posesión de la clave; luego la autorización se decide por fingerprint. No hay forma de saltarse la tabla de emparejados.

## Emparejamiento y código de verificación (SAS)

1. A abre TLS hacia B **sin pin** (B es desconocido). Ambos extremos conocen el certificado del otro.
2. Cada extremo calcula un **código de 8 dígitos** con el *exporter* de la sesión TLS (RFC 8446 §7.5, etiqueta `EXPERIMENTAL damebola pairing sas v1`, contexto = fingerprint del iniciador ‖ fingerprint del receptor). Ambos obtienen el mismo código; **el código nunca viaja por la red**.
3. Un intermediario que termine TLS con A y abra otra sesión con B produce **dos sesiones distintas** ⇒ códigos distintos en cada punta. Si los usuarios comparan el código en voz alta o por otro canal, lo detectan y cancelan. Hay una prueba automática de esta propiedad (`a_man_in_the_middle_yields_different_codes_on_each_leg`).
4. Ambos usuarios deben confirmar explícitamente: el receptor («Aceptar emparejamiento») y el iniciador («El código coincide»). Solo entonces cada lado guarda el fingerprint del otro (3 mensajes: `PAIR_REQUEST` → `PAIR_ACCEPT` → `PAIR_CONFIRM`). Cancelar, rechazar o que venza (2 minutos) no guarda nada.
5. A partir de ahí la credencial permanente es el **par de claves** (el fingerprint fijado), no un PIN. El código es de un solo uso.

Límites anti-abuso: una solicitud cada 10 s por dispositivo, máximo 3 emparejamientos simultáneos, el diálogo no se cierra con clic fuera.

Probabilidad de que un intermediario acierte el código por azar: 10⁻⁸ por intento, y cada intento requiere que una persona confirme.

## Recuperación de la identidad

La identidad es el par certificado (en SQLite) + clave privada (llavero o archivo). Al arrancar se comprueba que forman un par válido (la clave pública está en el certificado y rustls acepta el par). Resultados:

| Código | Qué significa | Qué hacer |
|---|---|---|
| `ok` | Clave y certificado coinciden. | Nada. |
| `store_unavailable` | El llavero (o el archivo) no respondió: bloqueado, sin permiso, sesión sin desbloquear. **Suele ser temporal.** | Desbloquear/aceptar el permiso y pulsar **Reintentar**. No se ofrece crear una identidad nueva. |
| `key_missing` | Hay certificado pero no se encontró su clave en ningún almacén. | Ver abajo. |
| `key_mismatch` | Hay una clave, pero no es la de este certificado (p. ej. otra instancia escribió su propia clave). | Ver abajo. |
| `cert_missing` | Hay emparejamientos (o una identidad archivada) pero falta el certificado. | Ver abajo. |
| `cert_corrupt` | El certificado guardado está dañado. | Ver abajo. |

En estos casos **la red no arranca, no se abre ningún puerto y no se modifica nada**; el resto de la aplicación (canciones, alertas locales, atajo) sigue funcionando.

Procedimiento, de menos a más destructivo:

1. **Reintentar** (no destructivo). Si el problema era un llavero bloqueado, vuelve la misma identidad y los emparejamientos siguen válidos.
2. **Buscar la clave original.** Si la tenías en otro almacén (p. ej. el llavero y ahora la app usa archivo, o al revés), volvé a arrancar con el almacén original: se encuentra sola, sin copiar nada. Una copia de seguridad de la base de datos (`damebola.db`) con el certificado correcto también sirve junto con la clave original.
3. **Crear identidad nueva** (explícito, con confirmación escrita `NUEVA IDENTIDAD`, solo si el problema no es temporal). Se genera otro par de claves en una **ranura nueva** (no toca ninguna clave existente, la vieja sigue intacta donde estuviera), el certificado anterior queda en la tabla `identity_archive` y los emparejamientos locales se conservan, **pero** los otros equipos tienen pinneado el certificado viejo y rechazarán a este: en **ambos** equipos hay que usar **Olvidar** y **Emparejar** de nuevo (comparando el código).

Sin una confirmación escrita, con una identidad sana o con un problema temporal, el backend rechaza el reemplazo.

### Aislamiento de instancias (evita el incidente original)

- Las variables de desarrollo `DAMEBOLA_KEY_STORAGE=file` y `DAMEBOLA_ALLOW_LOOPBACK=1` **solo se aplican** si además se define `DAMEBOLA_DATA_DIR` con una carpeta propia. Sin ella se ignoran (con un aviso): una instancia de prueba no puede forzar otro almacén de claves sobre los datos reales.
- Dos procesos no pueden usar la misma carpeta de datos a la vez (bloqueo exclusivo `instance.lock`); la segunda instancia no arranca.
- Las pruebas automáticas usan bases en memoria y almacenes de claves simulados: no tocan el llavero real.

Ejemplo de instancia de desarrollo aislada:

```bash
DAMEBOLA_DATA_DIR="$(mktemp -d)" DAMEBOLA_KEY_STORAGE=file npm run tauri dev
```

(Usa el puerto de red configurado: si hay otra instancia en 38473, cambiá el puerto en Dispositivos.)

## Revocación

«Olvidar» borra el registro local, envía `UNPAIR` si hay conexión y cierra. El otro equipo borra el suyo al recibirlo. Si el `UNPAIR` no llega (equipo apagado), ese equipo conservará el emparejamiento, pero el equipo que revocó rechazará (`unauthorized`) cualquier alerta suya y no volverá a conectarse con él. Para quedar limpio, olvidar también en el otro lado.

## Protección frente a mensajes remotos

- La alerta remota **no contiene datos**; no hay rutas, comandos ni nombres controlados por el remitente. El esquema es estricto y los campos desconocidos cierran la conexión.
- Reusa el sistema de alertas de la Fase 1 (misma pantalla, mismo volumen configurado por el destinatario; no hay bloqueo de teclado/ratón ni forma de impedir el cierre).
- Replays: dentro de una sesión, TLS garantiza integridad y orden; entre sesiones, un atacante no puede reinyectar (la sesión es nueva). Además se recuerdan los últimos 256 `id` durante 10 minutos y un duplicado no repite la acción.

## Superficie de red

- Solo se escucha en las **IPv4 privadas/link-local de interfaces activas** (no en `0.0.0.0`, no en IPv6). Los cambios de IP se detectan cada 5 s y se reajustan los listeners.
- Sin UPnP, sin apertura de puertos en routers, sin servidores externos, sin Internet. La conexión manual rechaza direcciones que no sean de red local.
- La red puede desactivarse por completo (el puerto se cierra).

## Límites conocidos

- Si los usuarios aprueban sin comparar el código, un intermediario activo en el primer emparejamiento no se detecta. Es el mismo modelo de confianza que Bluetooth «comparación numérica».
- El nombre y SO que se ven antes de emparejar provienen del mismo equipo remoto (mDNS/HELLO) y no están autenticados: sirven de pista, no de prueba.
- No se verifica caducidad del certificado (autofirmado, validez hasta 2124; la identidad es el fingerprint). No hay rotación de claves: reinstalar o regenerar la identidad exige emparejar de nuevo.
- Si la clave no está o no corresponde al certificado, **no** se genera una identidad nueva por sí sola (ver *Recuperación de la identidad*).
- Solo IPv4. En redes que usan rangos privados para VPN, esas interfaces también se consideran «LAN».
- Las pruebas manuales con el Keychain y el Credential Manager reales están pendientes (ver PHASE2_STATUS.md).
