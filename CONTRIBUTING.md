# Cómo contribuir

¡Gracias por querer ayudar! Es un proyecto humorístico y chico; las contribuciones son bienvenidas.

## Preparar el entorno

Ver los requisitos en el [README](README.md). Luego:

```bash
npm install
npm run tauri dev
```

## Antes de abrir un Pull Request

```bash
npm run typecheck
npm test
npm run test:rust
npm run build
```

La CI ejecuta lo mismo en macOS y Windows.

## Pautas

- Un cambio por PR, con una descripción clara de qué y por qué.
- Interfaz y mensajes de usuario en español.
- Sin permisos de sistema de archivos en el frontend: las operaciones con archivos van en Rust, con ids internos.
- Agregá pruebas para la lógica nueva (selección, ABM, validaciones).
- No subas canciones, bases de datos, claves ni archivos personales. Solo recursos propios o con licencia que permita redistribuirlos.
- Vulnerabilidades: ver [SECURITY.md](SECURITY.md), **no** en issues públicos.

## Reportar errores

Indicá sistema operativo, versión, pasos para reproducir y el comportamiento esperado.
