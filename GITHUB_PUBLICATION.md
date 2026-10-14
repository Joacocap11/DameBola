# Publicación en GitHub

- **Repositorio:** https://github.com/joacocap11/DameBola (público)
- **Rama principal:** `main`
- **Fecha de publicación:** 2026-10-14
- **Historial:** un único commit inicial limpio, creado desde cero (no existía historial Git previo). Autoría con la dirección `noreply` de GitHub.

## Auditoría de seguridad

Resultado: **superada**, sin hallazgos.

Revisado antes de crear el repositorio y de nuevo sobre los archivos en staging:

- Búsqueda de secretos (tokens `ghp_`/`gho_`, claves AWS, bloques `PRIVATE KEY`, `password`, `secret`, `api key`): sin coincidencias reales.
- Rutas personales, nombre de usuario local y volúmenes del equipo: sin coincidencias.
- IPs privadas (10.x, 192.168.x, 172.16–31.x) y dominios internos: ninguno.
- Archivos ocultos: solo `.gitignore` y `.github/`.
- Sin `.env`, claves, certificados, bases SQLite ni audio (MP3/WAV/OGG) en el árbol.
- Los archivos de bloqueo (`package-lock.json`, `Cargo.lock`) solo referencian los registros oficiales (npm y crates.io).
- Binarios incluidos: únicamente los iconos de la app (`src-tauri/icons`), generados desde `app-icon.svg`, ilustración propia.
- Los datos de prueba creados por la app durante las pruebas manuales ya habían sido borrados del directorio de datos del sistema (fuera del proyecto).

Limitación: **Gitleaks no estaba instalado**; se reemplazó por búsquedas con expresiones regulares sobre el árbol completo. Recomendado activar *secret scanning* y *push protection* en la configuración del repositorio.

## Archivos excluidos (`.gitignore`)

`node_modules/`, `dist/`, `target/`, `src-tauri/gen/`, `.env*` (salvo `.env.example`), claves y certificados (`*.pem`, `*.key`, `*.p12`, `*.pfx`, `*.cer`, `*.keystore`…), bases de datos (`*.db`, `*.sqlite*`, `-wal`, `-shm`), audio (`audio/`, `*.mp3`, `*.wav`, `*.ogg`, `*.flac`, `*.m4a`), instaladores (`*.dmg`, `*.app`, `*.msi`, `*.exe`…), logs, temporales, `.DS_Store`, `.idea/`, `.vscode/`. Se conservan `Cargo.lock` y `package-lock.json`.

También se eliminaron los iconos de Android/iOS generados por la herramienta (no se usan).

## Resultado de las pruebas (antes de publicar)

- `npm run typecheck`: correcto.
- `npm test`: 7 pruebas pasan.
- `cargo test --locked`: 25 pruebas pasan.
- `npm run build`: correcto.

## GitHub Actions

Workflow `CI` (`.github/workflows/ci.yml`), permisos `contents: read`, runners `macos-latest` y `windows-latest`: **exitoso en ambos** (instalación, typecheck, pruebas de frontend y Rust, build del frontend y compilación de la app Tauri sin instaladores).

## Advertencias pendientes

- La comunicación entre computadoras **no está implementada**.
- Windows: la CI confirma que compila y pasan las pruebas automáticas, pero **no hay prueba manual** en Windows.
- La CI no puede verificar audio real ni ventanas multimonitor.
- El inicio con el sistema no fue verificado.
- No hay capturas de pantalla publicadas (las disponibles mostraban el escritorio personal).
- Los instaladores no están firmados ni se publican releases.
