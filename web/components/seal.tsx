export function Seal({
  char = "深",
  size = "md",
  variant = "ink",
}: {
  char?: string;
  size?: "sm" | "md" | "lg";
  variant?: "ink" | "indigo";
}) {
  const dim = size === "sm" ? "w-7 h-7 text-sm" : size === "lg" ? "w-12 h-12 text-2xl" : "w-10 h-10 text-lg";
  const cls = variant === "indigo" ? "seal seal-indigo" : "seal";
  return (
    <span className={`${cls} ${dim}`} aria-hidden>
      {char}
    </span>
  );
}
