interface Props {
  size?: number;
}

// 앱 마크: 브랜드 그린 그라디언트 스퀘어클 + 상승 스파크라인.
// 브랜드 아이덴티티 유지를 위해 테마와 무관하게 그린 고정.
export function AppLogo({ size = 36 }: Props) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 36 36"
      fill="none"
      aria-hidden="true"
      style={{
        flexShrink: 0,
        pointerEvents: "none",
        filter: "drop-shadow(0 1.5px 4px rgba(0,0,0,.35))",
      }}
    >
      <defs>
        <linearGradient id="app-logo-bg" x1="0" y1="0" x2="36" y2="36" gradientUnits="userSpaceOnUse">
          <stop offset="0" stopColor="#57ab5a" />
          <stop offset="1" stopColor="#2da44e" />
        </linearGradient>
        <linearGradient id="app-logo-shade" x1="0" y1="0" x2="0" y2="36" gradientUnits="userSpaceOnUse">
          <stop offset="0.55" stopColor="rgba(0,0,0,0)" />
          <stop offset="1" stopColor="rgba(0,0,0,0.28)" />
        </linearGradient>
        <radialGradient id="app-logo-sheen" cx="0.3" cy="0.16" r="0.9">
          <stop offset="0" stopColor="rgba(255,255,255,0.42)" />
          <stop offset="0.45" stopColor="rgba(255,255,255,0.07)" />
          <stop offset="1" stopColor="rgba(255,255,255,0)" />
        </radialGradient>
      </defs>
      <rect x="1" y="1" width="34" height="34" rx="9.5" fill="url(#app-logo-bg)" />
      <rect x="1" y="1" width="34" height="34" rx="9.5" fill="url(#app-logo-shade)" />
      <rect x="1" y="1" width="34" height="34" rx="9.5" fill="url(#app-logo-sheen)" />
      <rect x="1.6" y="1.6" width="32.8" height="32.8" rx="9" stroke="rgba(255,255,255,0.3)" strokeWidth="0.9" />
      <rect x="8.6" y="21.5" width="4.2" height="6.5" rx="2.1" fill="#fff" opacity="0.5" />
      <rect x="15.9" y="17.5" width="4.2" height="10.5" rx="2.1" fill="#fff" opacity="0.68" />
      <rect x="23.2" y="13.5" width="4.2" height="14.5" rx="2.1" fill="#fff" opacity="0.88" />
      <path
        d="M8.5 15.5 L14.5 11.5 L19 13 L26.5 7.5"
        stroke="#fff"
        strokeWidth="2.2"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
      <circle cx="26.5" cy="7.5" r="3.6" fill="#fff" opacity="0.28" />
      <circle cx="26.5" cy="7.5" r="1.9" fill="#fff" />
    </svg>
  );
}
