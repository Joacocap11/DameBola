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
| Almacenamiento de la clave privada | **Llavero del sistema** (macOS Keychain / Windows Credential Manager) mediante la crate `keyring`. Si no está disponible, un archivo `device.key` con permisos `0600` (en Windows hereda la ACL del perfil del usuario; protección más débil, y la pantalla Dispositivos lo indica). Nunca en SQLite, nunca por la red, nunca en el repositorio. |
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
- Si el llavero no tiene la clave pero la base tiene un certificado (o no coinciden) se genera una identidad nueva; los otros equipos tendrán que volver a emparejar.
- Solo IPv4. En redes que usan rangos privados para VPN, esas interfaces también se consideran «LAN».
- Las pruebas manuales con el Keychain y el Credential Manager reales están pendientes (ver PHASE2_STATUS.md).
