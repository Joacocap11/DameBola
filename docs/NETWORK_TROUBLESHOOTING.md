# Red: firewall, descubrimiento y diagnóstico

Reemplazá `<IP>` por la dirección del otro equipo (ejemplos de documentación: `192.0.2.10`). No publiques las IP reales de tu red.

## Qué necesita DameBola

| Qué | Detalle |
|---|---|
| **TCP 38473** (configurable) entrante | Comunicación entre equipos (TLS + WebSocket). Es el único puerto que hay que permitir. |
| **UDP 5353 multicast** (mDNS) | Solo para el descubrimiento automático. Si está bloqueado, usá «Agregar dispositivo manualmente». |
| Misma red/subred | Sin routers que filtren entre equipos (aislamiento de clientes en Wi-Fi de invitados, VLAN separadas). |
| Privilegios | Ninguno de administrador para usar la app. Solo hacen falta para crear una regla de firewall a mano. |

DameBola **no modifica el firewall** por su cuenta ni abre puertos en el router, y no usa Internet.

## macOS

- La primera vez puede aparecer «¿Querés que "DameBola" acepte conexiones de red entrantes?» → **Permitir**. Si el firewall de macOS está activo: *Ajustes del Sistema → Red → Firewall → Opciones* y comprobá que DameBola esté en «Permitir conexiones entrantes».
- En macOS 15 o posterior puede pedir el permiso de **Red local** (*Ajustes → Privacidad y seguridad → Red local*). Sin él, el descubrimiento y las conexiones LAN fallan. En desarrollo (`npm run tauri dev`) el permiso lo recibe la app que lanzó el proceso (por ejemplo Terminal o iTerm).
- Comprobaciones:

```bash
# ¿Está escuchando DameBola?
lsof -nP -iTCP:38473 -sTCP:LISTEN
# ¿Alcanzo el puerto del otro equipo?
nc -vz <IP> 38473
# ¿Se anuncia el servicio mDNS?
dns-sd -B _damebola._tcp local.
```

## Windows 10/11

- Al primer arranque, Windows Defender Firewall pregunta qué redes permitir: marcá **Redes privadas**. Si la red está clasificada como **Pública**, Windows bloquea las conexiones entrantes: cambiala a «Privada» (*Configuración → Red e Internet → propiedades de la red*).
- Regla manual opcional (PowerShell **como administrador**, solo si querés crearla vos):

```powershell
New-NetFirewallRule -DisplayName "DameBola" -Direction Inbound -Protocol TCP -LocalPort 38473 -Profile Private -Action Allow
```

  Para quitarla: `Remove-NetFirewallRule -DisplayName "DameBola"`.
- Comprobaciones (PowerShell, sin administrador):

```powershell
# ¿Está escuchando DameBola?
Get-NetTCPConnection -LocalPort 38473 -State Listen
# ¿Alcanzo el puerto del otro equipo?
Test-NetConnection -ComputerName <IP> -Port 38473
```

## Comprobar conectividad entre una Mac y una PC

1. Conectá ambos equipos a la **misma red** (idealmente por la misma Wi-Fi o switch; evitá «redes de invitados»).
2. En cada uno abrí DameBola → **Dispositivos** y anotá la IP y el puerto de «Este equipo» (`192.0.2.x:38473`, por ejemplo).
3. Desde la Mac: `nc -vz <IP-de-la-PC> 38473`. Desde la PC: `Test-NetConnection <IP-de-la-Mac> -Port 38473` (`TcpTestSucceeded : True`).
   - Falla ⇒ firewall del destino, red pública (Windows), aislamiento de clientes en el router o puerto distinto.
4. En **Dispositivos** deberían aparecer mutuamente bajo «Descubiertos». Si no (mDNS bloqueado): **Agregar dispositivo manualmente** con la IP y el puerto del otro.
5. **Emparejar** desde uno: ambos ven el mismo código de 8 dígitos; compárenlo y confirmen en los dos.
6. Pulsá **DAME BOLA** en el dispositivo emparejado. Solo si la interfaz dice «La alerta se mostró en…» la alerta se mostró de verdad.

## Errores frecuentes

| Mensaje | Causa probable |
|---|---|
| «No hubo respuesta (tiempo agotado)…» | Firewall, red pública, otra subred, aislamiento de clientes, equipo dormido. |
| «Conexión rechazada…» | DameBola no está abierto/activado en el otro equipo o el puerto es otro. |
| «La identidad del dispositivo no coincide…» | El otro equipo reinstaló/regeneró su identidad, o alguien se hace pasar por él. Olvidalo y volvé a emparejar **comparando el código**. |
| «El puerto 38473 ya está en uso» | Otra aplicación usa el puerto: cambialo en la red local (en ambos equipos o al conectar manualmente). |
| Aparece «Conectado» y desaparece | Cambios de red/Wi-Fi, suspensión. Se reconecta sola con espera creciente (2 s → 60 s). |
| Cambió la IP | Se detecta en ≤ 5 s y se vuelve a anunciar; si el otro equipo sigue sin verte, pulsá **Conectar**. |
| Equipos con VPN o varias interfaces | Se escucha en todas las IPv4 privadas activas; si la VPN usa un rango privado, también se considera. Usá la IP correcta al conectar manualmente. |

## Errores de identidad (Dispositivos muestra una tarjeta roja)

La red no arranca si la identidad de este equipo no se puede verificar. **No se regenera nada por su cuenta.** Pasos en [PAIRING_SECURITY.md → Recuperación de la identidad](PAIRING_SECURITY.md#recuperación-de-la-identidad). Resumen: **Reintentar** primero (llavero bloqueado); solo si la clave realmente se perdió, «Crear identidad nueva» y volver a emparejar en ambos equipos.

## «Desconectado por vos»

Si pulsás **Desconectar**, el dispositivo no se reconecta solo (ni por mDNS, ni por un cambio de IP, ni al reiniciar la app) hasta que pulses **Conectar**. El otro equipo tampoco insistirá. Un corte involuntario (cable, Wi-Fi, suspensión) sí se recupera solo.

## Direcciones que se rechazan

DameBola solo se conecta a direcciones de la red local: privadas (`10.0.0.0/8`, `172.16.0.0/12`, `192.168.0.0/16`) y link-local (`169.254.0.0/16`). Se rechazan públicas, CGNAT/VPN compartida (`100.64.0.0/10`), `0.0.0.0`, multicast, broadcast, rangos de documentación/pruebas y reservados. El **descubrimiento automático** exige además que el equipo esté en la **misma subred** que una de tus interfaces (con varias interfaces vale cualquiera); la **conexión manual** y las direcciones ya guardadas de un equipo emparejado admiten otras subredes privadas (p. ej. otra VLAN enrutada). Si tu red usa un rango CGNAT (algunas VPN) no se aceptará: usá una dirección de la red local.

## Nombres de dispositivos

El nombre visible es tu **alias** si lo definiste (lápiz junto al dispositivo); si no, el último nombre que el propio equipo anunció (verificado por TLS). Un anuncio mDNS nunca cambia un nombre guardado.

## Desactivar la red

*Dispositivos → Red local* (interruptor). Cierra el puerto, las conexiones y el anuncio mDNS. «Recibir alertas de otros equipos» y «Silenciar» controlan solo las alertas, no la conectividad.
