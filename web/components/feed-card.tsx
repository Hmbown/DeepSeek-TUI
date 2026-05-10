import Link from "next/link";
import type { FeedItem } from "@/lib/types";
import { relativeTime } from "@/lib/github";

const KIND_LABEL: Record<FeedItem["kind"], { label: string; cn: string }> = {
  issue: { label: "Issue", cn: "议题" },
  pull: { label: "Pull", cn: "合并" },
  release: { label: "Release", cn: "发布" },
  discussion: { label: "Talk", cn: "讨论" },
};

function statePill(state: FeedItem["state"]) {
  const map: Record<FeedItem["state"], string> = {
    open: "pill pill-jade",
    closed: "pill pill-ghost",
    merged: "pill pill-hot",
    draft: "pill pill-ghost",
    published: "pill pill-ochre",
  };
  return <span className={map[state]}>{state}</span>;
}

export function FeedCard({ item, dense = false }: { item: FeedItem; dense?: boolean }) {
  const k = KIND_LABEL[item.kind];
  return (
    <article className={`hairline-b py-4 ${dense ? "" : "px-1"}`}>
      <div className="flex items-baseline gap-3 mb-1.5">
        <span className="font-mono text-[0.66rem] uppercase tracking-widest text-indigo">
          {k.label} <span className="font-cjk text-ink-mute normal-case tracking-normal ml-1">{k.cn}</span>
        </span>
        <span className="font-mono text-[0.7rem] text-ink-mute tabular">#{item.number}</span>
        <span className="ml-auto font-mono text-[0.7rem] text-ink-mute tabular">{relativeTime(item.updatedAt)}</span>
      </div>

      <h3 className="font-display text-base leading-snug">
        <Link href={item.url} className="hover:text-indigo transition-colors">
          {item.title}
        </Link>
      </h3>

      <div className="flex items-center gap-3 mt-2 flex-wrap">
        {statePill(item.state)}
        {item.labels.slice(0, 3).map((l) => (
          <span
            key={l.name}
            className="pill pill-ghost"
            style={{ borderColor: `#${l.color}`, color: `#${l.color}` }}
          >
            {l.name}
          </span>
        ))}
        <span className="ml-auto flex items-center gap-2 font-mono text-[0.7rem] text-ink-mute">
          <span>@{item.author}</span>
          {item.comments > 0 && <span className="tabular">· {item.comments} reply{item.comments === 1 ? "" : "s"}</span>}
        </span>
      </div>
    </article>
  );
}
