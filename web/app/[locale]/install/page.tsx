import Link from "next/link";
import { GITEE_ENABLED } from "@/lib/i18n/config";
import { Seal } from "@/components/seal";
import { InstallTabs } from "@/components/install-tabs";

export async function generateMetadata({ params }: { params: Promise<{ locale: string }> }) {
  const { locale } = await params;
  const isZh = locale === "zh";
  return {
    title: isZh ? "安装 · DeepSeek TUI" : "Install · DeepSeek TUI",
    description: isZh
      ? "在 macOS、Linux 或 Windows 上通过 Cargo、npm、Homebrew tap 或预编译二进制安装 deepseek-tui。"
      : "Install deepseek-tui on macOS, Linux, or Windows via Cargo, npm, the Homebrew tap, or pre-built binaries.",
  };
}

export default async function InstallPage({ params }: { params: Promise<{ locale: string }> }) {
  const { locale } = await params;
  const isZh = locale === "zh";

  return (
    <>
      {isZh ? (
        <>
          <section className="mx-auto max-w-[1400px] px-6 pt-12 pb-8">
            <div className="flex items-baseline gap-4 mb-3">
              <Seal char="装" />
              <div className="eyebrow">Section 01 · 安装</div>
            </div>
            <h1 className="font-display tracking-crisp">
              安装 <span className="font-cjk text-indigo text-5xl ml-2">Install</span>
            </h1>
            <p className="mt-5 max-w-3xl text-ink-soft text-lg leading-[1.9] tracking-wide">
              选择下方适合你平台的安装方式——首次加载时会自动检测。所有方式安装的都是同一个二进制文件：
              一个静态链接的 <code className="inline">deepseek</code> 可执行文件，交互使用时调用 TUI，同时暴露
              <code className="inline">doctor</code>、<code className="inline">mcp</code>、
              <code className="inline">serve</code>、<code className="inline">eval</code> 等子命令。
            </p>
          </section>

          <InstallTabs />

          {/* 国内镜像安装 */}
          <section className="mx-auto max-w-[1400px] px-6 py-16">
            <div className="flex items-baseline gap-4 mb-8 hairline-b pb-4">
              <Seal char="镜" />
              <h2 className="font-display">
                国内镜像安装 <span className="font-cjk text-ink-mute text-xl ml-2">中国大陆专用</span>
              </h2>
            </div>

            <div className="grid md:grid-cols-2 gap-0 col-rule hairline-t hairline-b min-w-0">
              {/* npmmirror */}
              <div className="p-6 min-w-0">
                <h3 className="font-display text-lg mb-2">npmmirror 镜像</h3>
                <p className="text-sm text-ink-soft leading-[1.9] tracking-wide mb-3">
                  将 npm 注册表切换至国内镜像，然后全局安装：
                </p>
                <pre className="code-block text-[0.78rem]">
{`npm config set registry https://registry.npmmirror.com
npm install -g deepseek-tui`}
                </pre>
              </div>

              {/* Tuna Cargo */}
              <div className="p-6 min-w-0">
                <h3 className="font-display text-lg mb-2">Tuna Cargo 镜像</h3>
                <p className="text-sm text-ink-soft leading-[1.9] tracking-wide mb-3">
                  在 <code className="inline">~/.cargo/config.toml</code> 中添加以下配置，即可使用清华 Tuna 源：
                </p>
                <pre className="code-block text-[0.78rem]">
{`[source.crates-io]
replace-with = "tuna"

[source.tuna]
registry = "sparse+https://mirrors.tuna.tsinghua.edu.cn/crates.io-index/"`}
                </pre>
                <p className="text-sm text-ink-soft leading-[1.9] tracking-wide mt-3">
                  配置完成后运行 <code className="inline">cargo install deepseek-tui-cli --locked</code> 即可（提供 <code className="inline">deepseek</code> 命令）。
                </p>
              </div>

              {/* Gitee 二进制 */}
              {GITEE_ENABLED && <div className="p-6 min-w-0">
                <h3 className="font-display text-lg mb-2">Gitee 预编译二进制</h3>
                <p className="text-sm text-ink-soft leading-[1.9] tracking-wide mb-3">
                  从 Gitee 发布页直接下载对应平台的预编译二进制文件，解压后即可使用：
                </p>
                <Link href="https://gitee.com/Hmbown/deepseek-tui/releases" className="font-mono text-[0.78rem] text-indigo hover:underline">
                  gitee.com/Hmbown/deepseek-tui/releases →
                </Link>
              </div>}

              {/* API 端点 */}
              <div className="p-6 min-w-0">
                <h3 className="font-display text-lg mb-2">国内 API 访问</h3>
                <p className="text-sm text-ink-soft leading-[1.9] tracking-wide mb-3">
                  DeepSeek 服务器位于中国境内，
                  <code className="inline">https://api.deepseek.com</code>{" "}
                  是
                  <a href="https://api-docs.deepseek.com/" className="body-link">官方唯一域名</a>，
                  国内可直连——无需替代节点。设置 API key：
                </p>
                <pre className="code-block text-[0.78rem]">
{`# 推荐：环境变量
export DEEPSEEK_API_KEY=sk-...

# 或保存到配置文件
deepseek auth set --provider deepseek --api-key sk-...`}
                </pre>
                <p className="text-sm text-ink-soft leading-[1.9] tracking-wide mt-3">
                  如有自建反向代理或私有镜像，可通过{" "}
                  <code className="inline">DEEPSEEK_BASE_URL</code> 覆盖默认地址。
                </p>
              </div>
            </div>

            {GITEE_ENABLED && <div className="mt-6">
              <Link href="https://gitee.com/Hmbown/deepseek-tui" className="body-link">
                Gitee 仓库镜像 →
              </Link>
            </div>}
          </section>

          {/* 安装后 */}
          <section className="mx-auto max-w-[1400px] px-6 py-16">
            <div className="flex items-baseline gap-4 mb-8 hairline-b pb-4">
              <Seal char="后" />
              <h2 className="font-display">
                安装之后 <span className="font-cjk text-ink-mute text-xl ml-2">下一步</span>
              </h2>
            </div>

            <ol className="grid md:grid-cols-3 gap-0 col-rule hairline-t hairline-b">
              <li className="p-6">
                <div className="font-display text-3xl text-indigo mb-2">①</div>
                <div className="eyebrow mb-2">获取密钥</div>
                <h3 className="font-display text-lg mb-2">在 platform.deepseek.com 注册</h3>
                <p className="text-sm text-ink-soft leading-[1.9] tracking-wide">
                  注册后会获得一个 <code className="inline">sk-...</code> 格式的 API 密钥。粘贴一次后，
                  <code className="inline"> deepseek auth</code> 会将其存储在
                  <code className="inline"> ~/.deepseek/config.toml</code>。
                </p>
              </li>
              <li className="p-6">
                <div className="font-display text-3xl text-indigo mb-2">②</div>
                <div className="eyebrow mb-2">运行诊断</div>
                <h3 className="font-display text-lg mb-2">验证环境</h3>
                <p className="text-sm text-ink-soft leading-[1.9] tracking-wide">
                  <code className="inline">deepseek doctor</code> 会检查密钥、网络连通性、沙箱可用性、
                  MCP 服务器，并将报告写入 <code className="inline">~/.deepseek/doctor.log</code>。
                </p>
              </li>
              <li className="p-6">
                <div className="font-display text-3xl text-indigo mb-2">③</div>
                <div className="eyebrow mb-2">试一试</div>
                <h3 className="font-display text-lg mb-2">第一个提示</h3>
                <p className="text-sm text-ink-soft leading-[1.9] tracking-wide">
                  <code className="inline">cd</code> 到某个项目目录，运行 <code className="inline">deepseek</code>，
                  然后提问：<em>"这个代码库是做什么的？"</em> Plan 模式默认只读——按
                  <kbd className="font-mono text-xs px-1 hairline-t hairline-b hairline-l hairline-r">Tab</kbd> 切换到 Agent 模式。
                </p>
              </li>
            </ol>
          </section>

          {/* 配置 */}
          <section className="bg-paper-deep hairline-t hairline-b">
            <div className="mx-auto max-w-[1400px] px-6 py-16 grid lg:grid-cols-12 gap-10 min-w-0">
              <div className="lg:col-span-5 min-w-0">
                <div className="eyebrow mb-3">配置文件 · Config</div>
                <h2 className="font-display text-3xl">文件存放位置</h2>
                <p className="mt-4 text-ink-soft leading-[1.9] tracking-wide">
                  所有配置存放在 <code className="inline">~/.deepseek/</code> 目录下。项目级别的覆盖通过仓库根目录的
                  <code className="inline">.deepseek/</code> 等项目级配置实现。
                </p>
                <div className="mt-6 space-y-3">
                  <Link href="/zh/docs" className="body-link inline-block">完整配置参考 →</Link>
                </div>
              </div>
              <div className="lg:col-span-7 min-w-0">
                <pre className="code-block text-[0.78rem]">
{`~/.deepseek/
├── config.toml          # API 密钥、模型、钩子、配置集
├── mcp.json             # MCP 服务器定义
├── skills/              # 用户技能（每个含 SKILL.md）
├── sessions/            # 检查点 + 离线队列
├── tasks/               # 后台任务存储
└── audit.log            # 凭证 / 审批 / 提权审计日志

# 项目级别
./.deepseek/             # 项目级配置（可选）`}
                </pre>
              </div>
            </div>
          </section>
        </>
      ) : (
        <>
          <section className="mx-auto max-w-[1400px] px-6 pt-12 pb-8">
            <div className="flex items-baseline gap-4 mb-3">
              <Seal char="装" />
              <div className="eyebrow">Section 01 · 安装</div>
            </div>
            <h1 className="font-display tracking-crisp">
              Install <span className="font-cjk text-indigo text-5xl ml-2">安装</span>
            </h1>
            <p className="mt-5 max-w-3xl text-ink-soft text-lg leading-relaxed">
              Pick your platform below — we auto-detect on first load. Every method gives you the same
              binary: a single static <code className="inline">deepseek</code> executable that
              dispatches to the TUI for interactive use and exposes subcommands like
              <code className="inline">doctor</code>, <code className="inline">mcp</code>,
              <code className="inline">serve</code>, <code className="inline">eval</code>.
            </p>
          </section>

          <InstallTabs />

          {/* AFTER INSTALL */}
          <section className="mx-auto max-w-[1400px] px-6 py-16">
            <div className="flex items-baseline gap-4 mb-8 hairline-b pb-4">
              <Seal char="后" />
              <h2 className="font-display">
                After install <span className="font-cjk text-ink-mute text-xl ml-2">下一步</span>
              </h2>
            </div>

            <ol className="grid md:grid-cols-3 gap-0 col-rule hairline-t hairline-b">
              <li className="p-6">
                <div className="font-display text-3xl text-indigo mb-2">①</div>
                <div className="eyebrow mb-2">Get a key</div>
                <h3 className="font-display text-lg mb-2">Sign up at platform.deepseek.com</h3>
                <p className="text-sm text-ink-soft leading-relaxed">
                  You'll get an <code className="inline">sk-...</code> API key. Paste it once and
                  <code className="inline"> deepseek auth</code> will store it in
                  <code className="inline"> ~/.deepseek/config.toml</code>.
                </p>
              </li>
              <li className="p-6">
                <div className="font-display text-3xl text-indigo mb-2">②</div>
                <div className="eyebrow mb-2">Run doctor</div>
                <h3 className="font-display text-lg mb-2">Verify your setup</h3>
                <p className="text-sm text-ink-soft leading-relaxed">
                  <code className="inline">deepseek doctor</code> checks your key, network,
                  sandbox availability, MCP servers, and writes a report to{" "}
                  <code className="inline">~/.deepseek/doctor.log</code>.
                </p>
              </li>
              <li className="p-6">
                <div className="font-display text-3xl text-indigo mb-2">③</div>
                <div className="eyebrow mb-2">Try it out</div>
                <h3 className="font-display text-lg mb-2">First prompt</h3>
                <p className="text-sm text-ink-soft leading-relaxed">
                  <code className="inline">cd</code> into a project, run <code className="inline">deepseek</code>,
                  and ask: <em>"What does this codebase do?"</em> Plan mode is read-only by default —
                  press <kbd className="font-mono text-xs px-1 hairline-t hairline-b hairline-l hairline-r">Tab</kbd> to switch to Agent mode.
                </p>
              </li>
            </ol>
          </section>

          {/* CONFIG */}
          <section className="bg-paper-deep hairline-t hairline-b">
            <div className="mx-auto max-w-[1400px] px-6 py-16 grid lg:grid-cols-12 gap-10 min-w-0">
              <div className="lg:col-span-5 min-w-0">
                <div className="eyebrow mb-3">Config files · 配置</div>
                <h2 className="font-display text-3xl">Where things live</h2>
                <p className="mt-4 text-ink-soft leading-relaxed">
                  All configuration goes under <code className="inline">~/.deepseek/</code>. Per-project
                  overrides via project-scoped <code className="inline">.deepseek/</code> config at the repo root.
                </p>
                <div className="mt-6 space-y-3">
                  <Link href="/docs" className="body-link inline-block">Full configuration reference →</Link>
                </div>
              </div>
              <div className="lg:col-span-7 min-w-0">
                <pre className="code-block text-[0.78rem]">
{`~/.deepseek/
├── config.toml          # api keys, model, hooks, profiles
├── mcp.json             # MCP server definitions
├── skills/              # user skills (each with SKILL.md)
├── sessions/            # checkpoints + offline queue
├── tasks/               # background task store
└── audit.log            # credential / approval / elevation audit trail

# project-local
./.deepseek/             # project-scoped config (optional)`}
                </pre>
              </div>
            </div>
          </section>
        </>
      )}
    </>
  );
}
