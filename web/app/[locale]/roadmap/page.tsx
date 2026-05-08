import Link from "next/link";
import { Seal } from "@/components/seal";
import { getCachedRoadmap, type RoadmapItem } from "@/lib/roadmap-feed";
import { getEnv } from "@/lib/kv";

export const revalidate = 1800;

export async function generateMetadata({ params }: { params: Promise<{ locale: string }> }) {
  const { locale } = await params;
  const isZh = locale === "zh";
  return {
    title: isZh ? "路线图 · DeepSeek TUI" : "Roadmap · DeepSeek TUI",
    description: isZh
      ? "已确认、正在评估和已排除的功能规划。"
      : "What's confirmed, what's being weighed, what's been ruled out for deepseek-tui.",
  };
}

const tracksEn = [
  {
    title: "Shipped",
    cn: "已完成",
    color: "jade",
    items: [
      { title: "13-crate workspace split", note: "core, app-server, tui, protocol, config, state, tools, mcp, hooks, execpolicy, agent, tui-core, cli" },
      { title: "Mode-gated tool registration", note: "Plan / Agent / YOLO with orthogonal approval modes" },
      { title: "MCP client + stdio server", note: "Bidirectional — both consume and expose tools" },
      { title: "Sandbox: seatbelt / landlock / AppContainer", note: "Per-platform with workspace boundary; Windows path is best-effort" },
      { title: "Background tasks + replayable timelines", note: "Durable task queue under ~/.deepseek/tasks/" },
      { title: "Runtime API (HTTP/SSE)", note: "deepseek serve --http with /v1/threads, /v1/tasks" },
      { title: "Sub-agent family", note: "agent_spawn / agent_wait / agent_result / agent_resume" },
      { title: "rlm tool", note: "Recursive long-context processing in sandboxed Python" },
    ],
  },
  {
    title: "Underway",
    cn: "进行中",
    color: "ochre",
    items: [
      { title: "Exa web-search backend", note: "Issue #431 — bundled alternative to the existing DDG + Bing path" },
      { title: "Feishu / Lark bot integration", note: "Issue #757 — chat frontend over the existing runtime API" },
      { title: "Responses API stabilization", note: "Currently behind EXPERIMENTAL_RESPONSES_API_ENV" },
    ],
  },
  {
    title: "Considered",
    cn: "考虑中",
    color: "cobalt",
    items: [
      { title: "Homebrew core formula", note: "Tap exists; pursuing homebrew-core inclusion" },
      { title: "Scoop manifest", note: "Mirror of Windows install path" },
      { title: "Native installer for Windows", note: "MSI / WinGet — pending" },
      { title: "First-class Tauri-based GUI shell", note: "Optional surface; TUI remains canonical" },
    ],
  },
  {
    title: "Ruled out",
    cn: "暂不考虑",
    color: "ink-mute",
    items: [
      { title: "Telemetry / phone-home", note: "Not while there's a single maintainer" },
      { title: "Hosted SaaS dashboard", note: "The terminal IS the dashboard" },
      { title: "Required login / accounts", note: "Bring your own API key, that's it" },
      { title: "Sponsored model recommendations", note: "Model picker stays neutral" },
    ],
  },
];

const tracksZh = [
  {
    title: "已完成",
    cn: "Shipped",
    color: "jade",
    items: [
      { title: "13 个 crate 的工作区拆分", note: "core, app-server, tui, protocol, config, state, tools, mcp, hooks, execpolicy, agent, tui-core, cli" },
      { title: "按模式注册工具", note: "Plan / Agent / YOLO，审批模式正交" },
      { title: "MCP 客户端 + stdio 服务器", note: "双向——既消费也暴露工具" },
      { title: "沙箱：seatbelt / landlock / AppContainer", note: "按平台隔离，含工作区边界；Windows 路径为尽力而为" },
      { title: "后台任务 + 可回放时间线", note: "持久化任务队列，位于 ~/.deepseek/tasks/" },
      { title: "运行时 API（HTTP/SSE）", note: "deepseek serve --http，暴露 /v1/threads、/v1/tasks" },
      { title: "子 Agent 体系", note: "agent_spawn / agent_wait / agent_result / agent_resume" },
      { title: "rlm 工具", note: "沙箱 Python 中的递归长上下文处理" },
    ],
  },
  {
    title: "进行中",
    cn: "Underway",
    color: "ochre",
    items: [
      { title: "Exa 网页搜索后端", note: "Issue #431——内建 Exa 路由，作为现有 DDG + Bing 路径的备选" },
      { title: "飞书 / Lark 机器人集成", note: "Issue #757——通过现有 runtime API 提供聊天前端" },
      { title: "Responses API 稳定化", note: "目前通过 EXPERIMENTAL_RESPONSES_API_ENV 启用" },
    ],
  },
  {
    title: "考虑中",
    cn: "Considered",
    color: "cobalt",
    items: [
      { title: "Homebrew 核心仓库", note: "Tap 已有；正在争取进入 homebrew-core" },
      { title: "Scoop 清单", note: "Windows 安装路径的镜像" },
      { title: "Windows 原生安装器", note: "MSI / WinGet——待定" },
      { title: "Tauri GUI 外壳", note: "可选界面；TUI 始终是正统" },
    ],
  },
  {
    title: "暂不考虑",
    cn: "Ruled out",
    color: "ink-mute",
    items: [
      { title: "遥测 / 回传数据", note: "只有一位维护者的情况下不会做" },
      { title: "托管 SaaS 面板", note: "终端本身就是面板" },
      { title: "强制登录 / 注册", note: "自带 API 密钥即可" },
      { title: "赞助商模型推荐", note: "模型选择器保持中立" },
    ],
  },
];

const colorFor = (c: string) =>
  c === "jade" ? "border-jade text-jade" :
  c === "ochre" ? "border-ochre text-ochre" :
  c === "cobalt" ? "border-cobalt text-cobalt" :
  "border-ink-mute text-ink-mute";

export default async function RoadmapPage({ params }: { params: Promise<{ locale: string }> }) {
  const { locale } = await params;
  const isZh = locale === "zh";
  const baseTracks = isZh ? tracksZh : tracksEn;

  // Live feed: shipped from GitHub Releases; underway/considered/ruled-out from issue labels.
  // Per-category fallback to the static items so unlabeled categories stay populated.
  let tracks = baseTracks;
  try {
    const env = await getEnv();
    const feed = await getCachedRoadmap(env.CURATED_KV, env.GITHUB_TOKEN);
    if (feed) {
      const liveByCategory: Record<string, RoadmapItem[]> = {
        Shipped: feed.shipped,
        Underway: feed.underway,
        Considered: feed.considered,
        "Ruled out": feed.ruledOut,
        已完成: feed.shipped,
        进行中: feed.underway,
        考虑中: feed.considered,
        暂不考虑: feed.ruledOut,
      };
      tracks = baseTracks.map((t) => {
        const live = liveByCategory[t.title];
        if (live && live.length > 0) {
          return { ...t, items: live.map((it) => ({ title: it.title, note: it.note })) };
        }
        return t;
      });
    }
  } catch {
    /* keep static fallback */
  }

  return (
    <>
      {isZh ? (
        <>
          <section className="mx-auto max-w-[1400px] px-6 pt-12 pb-8">
            <div className="flex items-baseline gap-4 mb-3">
              <Seal char="路" />
              <div className="eyebrow">Section 04 · 路线</div>
            </div>
            <h1 className="font-display tracking-crisp">
              路线图 <span className="font-cjk text-indigo text-5xl ml-2">Roadmap</span>
            </h1>
            <p className="mt-5 max-w-3xl text-ink-soft text-lg leading-[1.9] tracking-wide">
              已确认的功能、正在权衡的方案、以及已被排除的方向。未列在此页的内容均可在{" "}
              <Link href="https://github.com/Hmbown/deepseek-tui/discussions/new?category=ideas" className="body-link">
                Discussions
              </Link>{" "}
              中讨论。
            </p>
          </section>

          <section className="mx-auto max-w-[1400px] px-6 pb-20 grid lg:grid-cols-2 gap-px bg-paper-line">
            {tracks.map((t) => (
              <div key={t.title} className="bg-paper p-7">
                <div className={`hairline-b pb-3 mb-5 flex items-baseline justify-between border-b-2 ${colorFor(t.color)}`}>
                  <div>
                    <h2 className="font-display text-3xl">
                      {t.title} <span className="font-cjk text-2xl ml-2 text-ink-mute">{t.cn}</span>
                    </h2>
                  </div>
                  <div className="font-mono text-xs uppercase tracking-widest tabular text-ink-mute">{t.items.length} 项</div>
                </div>
                <ul className="space-y-4">
                  {t.items.map((it, i) => (
                    <li key={i} className="flex gap-4">
                      <span className={`font-display text-xl tabular shrink-0 w-8 ${colorFor(t.color)}`}>{String(i + 1).padStart(2, "0")}</span>
                      <div>
                        <div className="font-display text-base">{it.title}</div>
                        <div className="text-sm text-ink-soft mt-0.5 leading-[1.9] tracking-wide">{it.note}</div>
                      </div>
                    </li>
                  ))}
                </ul>
              </div>
            ))}
          </section>

          <section className="bg-ink text-paper">
            <div className="mx-auto max-w-[1400px] px-6 py-12 grid lg:grid-cols-12 gap-6 items-center">
              <div className="lg:col-span-8">
                <div className="font-cjk text-indigo text-lg mb-2">参与塑造</div>
                <h2 className="font-display text-paper text-3xl">想影响这份清单？</h2>
                <p className="mt-3 text-paper-deep/80 leading-[1.9] tracking-wide max-w-2xl">
                  路线图反映的是维护者的计划——但 PR 和有理有据的讨论会不断调整优先级。
                  带一个可运行的原型来，"考虑中"就能变成"进行中"。
                </p>
              </div>
              <div className="lg:col-span-4 flex flex-col gap-3">
                <Link
                  href="https://github.com/Hmbown/deepseek-tui/discussions/new?category=ideas"
                  className="px-5 py-3 bg-indigo text-paper font-mono text-sm uppercase tracking-wider text-center hover:bg-indigo-deep transition-colors"
                >
                  提交想法 →
                </Link>
                <Link
                  href="https://github.com/Hmbown/deepseek-tui/issues?q=is%3Aissue+is%3Aopen+label%3A%22good+first+issue%22"
                  className="px-5 py-3 hairline-t hairline-b hairline-l hairline-r border-paper-deep/30 font-mono text-sm uppercase tracking-wider text-center hover:bg-paper hover:text-ink transition-colors"
                >
                  Good first issues →
                </Link>
              </div>
            </div>
          </section>
        </>
      ) : (
        <>
          <section className="mx-auto max-w-[1400px] px-6 pt-12 pb-8">
            <div className="flex items-baseline gap-4 mb-3">
              <Seal char="路" />
              <div className="eyebrow">Section 04 · 路线</div>
            </div>
            <h1 className="font-display tracking-crisp">
              Roadmap <span className="font-cjk text-indigo text-5xl ml-2">路线图</span>
            </h1>
            <p className="mt-5 max-w-3xl text-ink-soft text-lg leading-relaxed">
              What's confirmed, what's being weighed, what's been ruled out. Anything not on this page
              is fair game for{" "}
              <Link href="https://github.com/Hmbown/deepseek-tui/discussions/new?category=ideas" className="body-link">
                discussion
              </Link>.
            </p>
          </section>

          <section className="mx-auto max-w-[1400px] px-6 pb-20 grid lg:grid-cols-2 gap-px bg-paper-line">
            {tracks.map((t) => (
              <div key={t.title} className="bg-paper p-7">
                <div className={`hairline-b pb-3 mb-5 flex items-baseline justify-between border-b-2 ${colorFor(t.color)}`}>
                  <div>
                    <h2 className="font-display text-3xl">
                      {t.title} <span className="font-cjk text-2xl ml-2 text-ink-mute">{t.cn}</span>
                    </h2>
                  </div>
                  <div className="font-mono text-xs uppercase tracking-widest tabular text-ink-mute">{t.items.length} items</div>
                </div>
                <ul className="space-y-4">
                  {t.items.map((it, i) => (
                    <li key={i} className="flex gap-4">
                      <span className={`font-display text-xl tabular shrink-0 w-8 ${colorFor(t.color)}`}>{String(i + 1).padStart(2, "0")}</span>
                      <div>
                        <div className="font-display text-base">{it.title}</div>
                        <div className="text-sm text-ink-soft mt-0.5 leading-relaxed">{it.note}</div>
                      </div>
                    </li>
                  ))}
                </ul>
              </div>
            ))}
          </section>

          <section className="bg-ink text-paper">
            <div className="mx-auto max-w-[1400px] px-6 py-12 grid lg:grid-cols-12 gap-6 items-center">
              <div className="lg:col-span-8">
                <div className="font-cjk text-indigo text-lg mb-2">参与塑造</div>
                <h2 className="font-display text-paper text-3xl">Want to shape this list?</h2>
                <p className="mt-3 text-paper-deep/80 leading-relaxed max-w-2xl">
                  The roadmap reflects what the maintainer plans to do — but PRs and well-argued
                  discussions reorder it constantly. Show up with a working prototype and watch
                  "Considered" become "Underway".
                </p>
              </div>
              <div className="lg:col-span-4 flex flex-col gap-3">
                <Link
                  href="https://github.com/Hmbown/deepseek-tui/discussions/new?category=ideas"
                  className="px-5 py-3 bg-indigo text-paper font-mono text-sm uppercase tracking-wider text-center hover:bg-indigo-deep transition-colors"
                >
                  Propose an idea →
                </Link>
                <Link
                  href="https://github.com/Hmbown/deepseek-tui/issues?q=is%3Aissue+is%3Aopen+label%3A%22good+first+issue%22"
                  className="px-5 py-3 hairline-t hairline-b hairline-l hairline-r border-paper-deep/30 font-mono text-sm uppercase tracking-wider text-center hover:bg-paper hover:text-ink transition-colors"
                >
                  Good first issues →
                </Link>
              </div>
            </div>
          </section>
        </>
      )}
    </>
  );
}
