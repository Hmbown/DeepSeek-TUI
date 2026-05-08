/**
 * Stylized whale mark — a nod to DeepSeek's cetacean motif.
 * Kept minimal and geometric so it reads as a wordmark accent, not an illustration.
 */
export function Whale({ size = 22, className = "" }: { size?: number; className?: string }) {
  return (
    <svg
      viewBox="0 0 64 32"
      width={size}
      height={(size * 32) / 64}
      className={className}
      aria-hidden
      fill="currentColor"
    >
      {/* body */}
      <path d="M2 18 C 2 10, 14 4, 28 4 C 42 4, 50 10, 50 16 C 50 22, 42 28, 28 28 C 18 28, 8 24, 2 18 Z" />
      {/* tail flukes */}
      <path d="M48 12 L 62 4 L 58 16 L 62 28 L 48 20 Z" />
      {/* eye */}
      <circle cx="14" cy="14" r="1.4" fill="#FFFFFF" />
      {/* spout */}
      <path d="M22 4 L 22 0 M 26 4 L 28 0 M 18 4 L 16 0" stroke="currentColor" strokeWidth="1.2" fill="none" />
    </svg>
  );
}
