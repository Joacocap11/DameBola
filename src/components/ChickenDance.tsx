import { CHICKEN_GIF } from "../config/alertMedia";

interface Props {
  animated?: boolean;
  className?: string;
}

/**
 * Pollo bailarín 100 % propio (SVG + CSS), sin recursos externos.
 * Si `CHICKEN_GIF` está definido en `config/alertMedia.ts` se usa ese GIF.
 */
export function ChickenDance({ animated = true, className = "" }: Props) {
  if (CHICKEN_GIF && animated) {
    return <img src={CHICKEN_GIF} alt="Pollo bailando" className={className} draggable={false} />;
  }
  return (
    <div className={`chicken ${animated ? "is-animated" : ""} ${className}`} role="img" aria-label="Pollo bailando">
      <svg viewBox="0 0 400 420" className="h-full w-full">
        <g className="ck-leg-l">
          <path d="M165 320v70M165 390l-26 8M165 390h24M165 390l20 12" stroke="#f59e0b" strokeWidth="12" strokeLinecap="round" strokeLinejoin="round" fill="none" />
        </g>
        <g className="ck-leg-r">
          <path d="M235 320v70M235 390l-26 8M235 390h24M235 390l20 12" stroke="#f59e0b" strokeWidth="12" strokeLinecap="round" strokeLinejoin="round" fill="none" />
        </g>
        <g className="ck-body">
          <g className="ck-tail">
            <path d="M290 250c40-60 80-70 100-50-10 20-30 30-45 36 25 0 40 14 35 30-30 8-65 0-90-16z" fill="#fca5a5" />
          </g>
          <ellipse cx="200" cy="250" rx="115" ry="100" fill="#fff7ed" />
          <g className="ck-wing-l">
            <path d="M110 220c-50-10-80 20-80 50 40 10 80-5 96-40z" fill="#fed7aa" />
          </g>
          <g className="ck-wing-r">
            <path d="M290 220c50-10 80 20 80 50-40 10-80-5-96-40z" fill="#fed7aa" />
          </g>
          <g className="ck-head">
            <circle cx="200" cy="130" r="72" fill="#fff7ed" />
            <path d="M160 70c-8-34 20-48 36-26 10-26 46-18 44 16 14-6 28 6 22 22-30-10-54-6-70 14-14-16-24-22-32-26z" fill="#b91c1c" />
            <path d="M165 135l-40 18 42 10z" fill="#f59e0b" />
            <path d="M172 165c-2 26 12 40 26 28-4-14-6-26-12-36z" fill="#b91c1c" />
            <circle cx="205" cy="118" r="12" fill="#1f2937" />
            <circle cx="209" cy="113" r="4" fill="#fff" />
          </g>
        </g>
      </svg>
    </div>
  );
}
