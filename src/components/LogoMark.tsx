/** The app mark: a branch whose tip sits inside a protection ring. */
export function LogoMark({ size = 32, title }: { size?: number; title?: string }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="64 64 896 896"
      role={title ? "img" : undefined}
      aria-hidden={title ? undefined : true}
      aria-label={title}
      className="logo-mark"
    >
      <defs>
        <linearGradient id="lm-tile" x1="0" y1="0" x2="0.35" y2="1">
          <stop offset="0" stopColor="#1E2836" />
          <stop offset="1" stopColor="#0A0E15" />
        </linearGradient>
        <linearGradient id="lm-ring" x1="0" y1="0" x2="1" y2="1">
          <stop offset="0" stopColor="#8BEAF2" />
          <stop offset="1" stopColor="#36AFC2" />
        </linearGradient>
      </defs>
      <rect x="64" y="64" width="896" height="896" rx="208" fill="url(#lm-tile)" />
      <rect x="65.5" y="65.5" width="893" height="893" rx="206.5" fill="none" stroke="#fff" strokeOpacity="0.09" strokeWidth="3" />
      <g fill="none" stroke="#C7D2DE" strokeWidth="64" strokeLinecap="round">
        <path d="M400 800 V300" />
        <path d="M400 652 C400 528 624 560 624 432" />
      </g>
      <circle cx="400" cy="272" r="70" fill="#C7D2DE" />
      <circle cx="624" cy="384" r="62" fill="#5CD0DC" />
      <circle
        cx="624"
        cy="384"
        r="134"
        fill="none"
        stroke="url(#lm-ring)"
        strokeWidth="38"
        strokeLinecap="round"
        strokeDasharray="682 160"
        strokeDashoffset="560"
      />
    </svg>
  );
}
