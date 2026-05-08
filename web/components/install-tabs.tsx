"use client";

import { useEffect, useState } from "react";

type OS = "macos" | "linux" | "windows" | "any";

interface Method {
  id: string;
  os: OS;
  label: string;
  cn: string;
  recommended?: boolean;
  comingSoon?: boolean;
  prereq: string;
  cmd: string;
}

const METHODS: Method[] = [
  // ─── macOS ────────────────────────────────────────────────
  {
    id: "cargo-mac",
    os: "macos",
    label: "Cargo (recommended)",
    cn: "Cargo · 推荐",
    recommended: true,
    prereq: "Rust 1.88+ — install via rustup.rs if needed",
    cmd: `# Install the dispatcher (provides \`deepseek\`)
cargo install deepseek-tui-cli --locked

# Optional: also install the raw TUI binary (\`deepseek-tui\`)
cargo install deepseek-tui --locked

# Set your API key (one-time)
export DEEPSEEK_API_KEY=sk-...
echo 'export DEEPSEEK_API_KEY=sk-...' >> ~/.zshrc

# Run it
deepseek`,
  },
  {
    id: "npm-mac",
    os: "macos",
    label: "npm wrapper",
    cn: "npm 包",
    prereq: "Node.js 18+",
    cmd: `npm install -g deepseek-tui

# Provides both binaries on PATH:
deepseek          # canonical dispatcher
deepseek-tui      # raw TUI binary`,
  },
  {
    id: "binary-mac",
    os: "macos",
    label: "Pre-built binary",
    cn: "二进制",
    prereq: "Apple Silicon (arm64) or Intel (x64). Releases ship raw binaries — no archive to extract.",
    cmd: `# Apple Silicon
curl -fsSL -o deepseek \\
  https://github.com/Hmbown/deepseek-tui/releases/latest/download/deepseek-macos-arm64
chmod +x deepseek
xattr -d com.apple.quarantine deepseek 2>/dev/null || true
sudo mv deepseek /usr/local/bin/

# Intel
curl -fsSL -o deepseek \\
  https://github.com/Hmbown/deepseek-tui/releases/latest/download/deepseek-macos-x64
chmod +x deepseek
xattr -d com.apple.quarantine deepseek 2>/dev/null || true
sudo mv deepseek /usr/local/bin/

# Verify checksum (optional but recommended)
curl -fsSL -O https://github.com/Hmbown/deepseek-tui/releases/latest/download/deepseek-artifacts-sha256.txt
shasum -a 256 -c deepseek-artifacts-sha256.txt --ignore-missing

deepseek`,
  },
  {
    id: "brew",
    os: "macos",
    label: "Homebrew",
    cn: "Homebrew",
    prereq: "Homebrew on macOS or Linux; installs the dispatcher and companion TUI from the official Hmbown tap.",
    cmd: `brew tap Hmbown/deepseek-tui
brew install deepseek-tui

deepseek --version
deepseek`,
  },

  // ─── Linux ────────────────────────────────────────────────
  {
    id: "cargo-linux",
    os: "linux",
    label: "Cargo (recommended)",
    cn: "Cargo · 推荐",
    recommended: true,
    prereq: "Rust 1.88+; on Debian/Ubuntu: apt install build-essential pkg-config libssl-dev",
    cmd: `cargo install deepseek-tui-cli --locked
export DEEPSEEK_API_KEY=sk-...
deepseek`,
  },
  {
    id: "npm-linux",
    os: "linux",
    label: "npm wrapper",
    cn: "npm 包",
    prereq: "Node.js 18+",
    cmd: `npm install -g deepseek-tui
deepseek`,
  },
  {
    id: "binary-linux",
    os: "linux",
    label: "Pre-built binary",
    cn: "二进制",
    prereq: "x86_64 or aarch64 glibc. Releases ship raw binaries — no archive to extract.",
    cmd: `# x86_64
curl -fsSL -o deepseek \\
  https://github.com/Hmbown/deepseek-tui/releases/latest/download/deepseek-linux-x64
chmod +x deepseek
sudo mv deepseek /usr/local/bin/

# arm64
curl -fsSL -o deepseek \\
  https://github.com/Hmbown/deepseek-tui/releases/latest/download/deepseek-linux-arm64
chmod +x deepseek
sudo mv deepseek /usr/local/bin/

# Verify checksum (optional but recommended)
curl -fsSL -O https://github.com/Hmbown/deepseek-tui/releases/latest/download/deepseek-artifacts-sha256.txt
sha256sum -c deepseek-artifacts-sha256.txt --ignore-missing

deepseek`,
  },

  // ─── Windows ──────────────────────────────────────────────
  {
    id: "cargo-win",
    os: "windows",
    label: "Cargo (recommended)",
    cn: "Cargo · 推荐",
    recommended: true,
    prereq: "Rust 1.88+ via rustup-init.exe",
    cmd: `cargo install deepseek-tui-cli --locked
$env:DEEPSEEK_API_KEY = "sk-..."
deepseek`,
  },
  {
    id: "npm-win",
    os: "windows",
    label: "npm wrapper",
    cn: "npm 包",
    prereq: "Node.js 18+",
    cmd: `npm install -g deepseek-tui
deepseek`,
  },
  {
    id: "binary-win",
    os: "windows",
    label: "Pre-built binary",
    cn: "二进制",
    prereq: "Windows 10+ x64. Releases ship a raw .exe — no archive to extract.",
    cmd: `# PowerShell
$ErrorActionPreference = "Stop"
$dest = "$Env:USERPROFILE\\bin"
New-Item -ItemType Directory -Force $dest | Out-Null

Invoke-WebRequest \`
  -Uri https://github.com/Hmbown/deepseek-tui/releases/latest/download/deepseek-windows-x64.exe \`
  -OutFile "$dest\\deepseek.exe"

# Add to PATH for this session (persist via System Properties → Environment Variables)
$Env:Path = "$dest;$Env:Path"

$Env:DEEPSEEK_API_KEY = "sk-..."
deepseek`,
  },
  {
    id: "scoop",
    os: "windows",
    label: "Scoop",
    cn: "Scoop",
    comingSoon: true,
    prereq: "Scoop manifest not yet published — use Cargo or the pre-built .exe above.",
    cmd: `# Coming soon — no Scoop manifest yet.
# Working alternatives on Windows:
#   - Cargo (recommended above)
#   - Pre-built deepseek-windows-x64.exe (above)
#
# Track progress:
#   https://github.com/Hmbown/deepseek-tui/issues`,
  },

  // ─── Any (cross-platform) ────────────────────────────────
  {
    id: "docker",
    os: "any",
    label: "Docker",
    cn: "Docker",
    prereq: "Dockerfile ships with the repo (multi-arch buildx). No prebuilt image is published to a registry yet.",
    cmd: `git clone https://github.com/Hmbown/deepseek-tui
cd deepseek-tui

# Build for your local arch
docker build -t deepseek-tui .

# Or multi-arch via buildx
docker buildx build --platform linux/amd64,linux/arm64 -t deepseek-tui .

# Run interactively, mounting your config + a project
docker run --rm -it \\
  -e DEEPSEEK_API_KEY=$DEEPSEEK_API_KEY \\
  -v ~/.deepseek:/home/deepseek/.deepseek \\
  -v "$PWD:/work" -w /work \\
  deepseek-tui`,
  },
  {
    id: "from-source",
    os: "any",
    label: "Build from source",
    cn: "源码编译",
    prereq: "Rust 1.88+ and a git checkout — useful for hacking on the workspace itself.",
    cmd: `git clone https://github.com/Hmbown/deepseek-tui
cd deepseek-tui

# Builds both \`deepseek\` and \`deepseek-tui\` into ./target/release/
cargo build --release --locked

# Run without installing
./target/release/deepseek

# Or install both binaries from your local checkout
cargo install --path crates/cli --locked   # provides \`deepseek\`
cargo install --path crates/tui --locked   # provides \`deepseek-tui\``,
  },
];

const OS_LABEL: Record<OS, { en: string; cn: string }> = {
  macos: { en: "macOS", cn: "苹果" },
  linux: { en: "Linux", cn: "Linux" },
  windows: { en: "Windows", cn: "视窗" },
  any: { en: "Any platform", cn: "通用" },
};

function detectOS(): OS {
  if (typeof navigator === "undefined") return "macos";
  const ua = navigator.userAgent.toLowerCase();
  if (ua.includes("mac")) return "macos";
  if (ua.includes("win")) return "windows";
  if (ua.includes("linux")) return "linux";
  return "macos";
}

export function InstallTabs() {
  const [os, setOS] = useState<OS>("macos");
  const [copied, setCopied] = useState<string | null>(null);

  useEffect(() => { setOS(detectOS()); }, []);

  // Show OS-specific methods + universal ones (Docker status / source build).
  // On the "Any" tab, only show universal ones.
  const methods = METHODS.filter((m) => (os === "any" ? m.os === "any" : m.os === os || m.os === "any"));

  const copy = (id: string, text: string) => {
    navigator.clipboard?.writeText(text);
    setCopied(id);
    setTimeout(() => setCopied(null), 1400);
  };

  return (
    <div>
      {/* OS selector */}
      <div className="hairline-t hairline-b grid grid-cols-4">
        {(["macos", "linux", "windows", "any"] as OS[]).map((o) => {
          const active = os === o;
          return (
            <button
              key={o}
              onClick={() => setOS(o)}
              className={`px-4 py-4 text-left transition-colors hairline-l first:border-l-0 ${
                active ? "bg-ink text-paper" : "bg-paper hover:bg-paper-deep"
              }`}
            >
              <div className={`eyebrow mb-1 ${active ? "text-paper-deep/70" : ""}`}>
                {active ? "▼ " : ""}Detected · {o === detectOS() ? "auto" : "switch"}
              </div>
              <div className="font-display text-lg leading-tight">{OS_LABEL[o].en}</div>
              <div className={`font-cjk text-xs ${active ? "text-paper-deep/80" : "text-ink-mute"}`}>
                {OS_LABEL[o].cn}
              </div>
            </button>
          );
        })}
      </div>

      {/* methods */}
      <div className="hairline-b">
        {methods.map((m, i) => (
          <div key={m.id} className={i > 0 ? "hairline-t" : ""}>
            <div className="grid lg:grid-cols-12 gap-0 min-w-0">
              <div className={`min-w-0 lg:col-span-4 p-6 hairline-r-0 lg:hairline-r bg-paper ${m.comingSoon ? "opacity-70" : ""}`}>
                <div className="flex items-center gap-2 mb-2 flex-wrap">
                  {m.recommended && <span className="pill pill-hot">Recommended</span>}
                  {m.comingSoon && <span className="pill pill-ghost">Coming soon</span>}
                  <span className="eyebrow">Method 0{i + 1}</span>
                </div>
                <h3 className="font-display text-xl mb-1">{m.label}</h3>
                <div className="font-cjk text-sm text-ink-mute mb-3">{m.cn}</div>
                <div className="text-xs text-ink-soft leading-relaxed">
                  <strong className="text-ink">Prereq:</strong> {m.prereq}
                </div>
              </div>
              <div className={`min-w-0 lg:col-span-8 p-6 bg-paper-deep relative ${m.comingSoon ? "opacity-80" : ""}`}>
                {!m.comingSoon && (
                  <button
                    onClick={() => copy(m.id, m.cmd)}
                    className="absolute top-7 right-7 z-10 px-3 py-1 bg-paper hairline-t hairline-b hairline-l hairline-r font-mono text-[0.7rem] uppercase tracking-wider hover:bg-indigo hover:text-paper transition-colors"
                  >
                    {copied === m.id ? "Copied ✓" : "Copy"}
                  </button>
                )}
                <pre className="code-block text-[0.78rem] m-0 max-w-full">{m.cmd}</pre>
              </div>
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}
