import type { RepoStats } from "@/lib/types";

function fmt(n: number): string {
  if (n >= 1000) return (n / 1000).toFixed(1) + "k";
  return n.toString();
}

export function StatGrid({ stats }: { stats: RepoStats }) {
  const cells = [
    { label: "Stars", cn: "星标", value: fmt(stats.stars) },
    { label: "Forks", cn: "复刻", value: fmt(stats.forks) },
    { label: "Contributors", cn: "贡献者", value: fmt(stats.contributors) },
    {
      label: "Latest",
      cn: "版本",
      value: stats.latestRelease?.tag ?? "—",
      mono: true,
    },
  ];

  return (
    <div className="hairline-t hairline-b grid grid-cols-2 sm:grid-cols-4 col-rule">
      {cells.map((c) => (
        <div key={c.label} className="px-5 py-5">
          <div className="eyebrow mb-2">
            {c.label} · <span className="font-cjk normal-case tracking-normal">{c.cn}</span>
          </div>
          <div className={c.mono ? "font-mono text-2xl tabular text-ink" : "bignum text-ink"}>{c.value}</div>
        </div>
      ))}
    </div>
  );
}
