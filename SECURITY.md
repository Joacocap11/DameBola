# Política de seguridad

## Reportar una vulnerabilidad

**No abras un issue público** para reportar vulnerabilidades.

Usá el reporte privado de GitHub: pestaña **Security → Report a vulnerability** de este repositorio
(<https://github.com/joacocap11/DameBola/security/advisories/new>).

Incluí, si podés: versión o commit, sistema operativo, pasos para reproducirlo e impacto estimado.
No incluyas datos personales ni archivos de terceros.

Se responderá lo antes posible; es un proyecto personal mantenido sin garantías de plazo.

## Alcance

DameBola es una aplicación local (Fase 1). Son de particular interés: acceso a archivos fuera del directorio
administrado, ejecución de código al importar audio, y cualquier ampliación de permisos del frontend
(`src-tauri/capabilities/`).

## Versiones soportadas

Solo la última versión de la rama `main`.
