/**
 * Medio animado que se muestra en la alerta.
 *
 * Por defecto se usa la animación propia en SVG + CSS (`ChickenDance`),
 * sin recursos externos ni licencias de terceros.
 *
 * Para usar un GIF propio:
 *   1. Copiar el archivo a `src/assets/` (p. ej. `pollo.gif`) con una licencia que lo permita.
 *   2. Importarlo aquí:  import pollo from "../assets/pollo.gif";
 *   3. Asignar:          export const CHICKEN_GIF: string | null = pollo;
 * Vite lo empaqueta dentro de la aplicación: no se descarga nada en ejecución.
 */
export const CHICKEN_GIF: string | null = null;
