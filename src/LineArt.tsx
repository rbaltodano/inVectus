// Thin line diagrams for empty states and section headers. Decorative only: they name a concept
// (regions, overlap, coverage), never show data, so they are hidden from assistive technology.

type Kind = "sphere" | "venn" | "rings" | "orbit" | "grid";

const STROKE = { fill: "none", stroke: "currentColor", strokeWidth: 1, vectorEffect: "non-scaling-stroke" } as const;

export default function LineArt({ kind, size = 72 }: { kind: Kind; size?: number }) {
  return <svg className="line-art" width={size} height={size} viewBox="0 0 100 100" aria-hidden="true">
    {kind === "sphere" && <g {...STROKE}>
      <circle cx="50" cy="50" r="40" />
      <ellipse cx="50" cy="50" rx="40" ry="13" />
      <ellipse cx="50" cy="50" rx="15" ry="40" opacity="0.6" />
      <ellipse cx="50" cy="50" rx="30" ry="40" opacity="0.35" />
      <ellipse cx="50" cy="50" rx="40" ry="27" opacity="0.35" strokeDasharray="2 3" />
    </g>}
    {kind === "venn" && <g {...STROKE}>
      <circle cx="38" cy="50" r="26" />
      <circle cx="62" cy="50" r="26" />
      <path d="M50 27 L50 73" strokeDasharray="2 3" opacity="0.6" />
      {[34, 40, 46, 52, 58, 64].map((y) => <path key={y} d={`M${50 - Math.sqrt(Math.max(0, 26 * 26 - (y - 50) ** 2)) + 12} ${y} L${50 + Math.sqrt(Math.max(0, 26 * 26 - (y - 50) ** 2)) - 12} ${y}`} opacity="0.5" />)}
    </g>}
    {kind === "rings" && <g {...STROKE}>
      {[40, 30, 20, 10].map((r, i) => <circle key={r} cx="50" cy="50" r={r} opacity={1 - i * 0.2} />)}
      <path d="M50 4 L50 96 M4 50 L96 50" strokeDasharray="2 4" opacity="0.4" />
    </g>}
    {kind === "orbit" && <g {...STROKE}>
      <circle cx="50" cy="50" r="7" />
      <ellipse cx="50" cy="50" rx="40" ry="16" transform="rotate(-20 50 50)" />
      <ellipse cx="50" cy="50" rx="28" ry="10" transform="rotate(25 50 50)" opacity="0.6" />
      <circle cx="86" cy="36" r="2.5" fill="currentColor" />
      <circle cx="27" cy="57" r="2" fill="currentColor" opacity="0.6" />
    </g>}
    {kind === "grid" && <g {...STROKE}>
      <path d="M14 86 L14 14 M14 86 L86 86" />
      {[26, 38, 50, 62, 74].map((v) => <path key={v} d={`M${v} 18 L${v} 86 M14 ${v} L82 ${v}`} opacity="0.35" />)}
      <path d="M82 86 L86 86 M14 18 L14 14" />
    </g>}
  </svg>;
}
