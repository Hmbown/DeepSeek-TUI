# 安装 DeepSeek TUI

本页面涵盖所有受支持的安装方式，以及最常见的"安装失败"问题，包括 **Linux ARM64** 和其他不太常见的平台。

如果你只需要简短的版本，请参阅
[主 README](../README.md#quickstart) 或
[简体中文 README](../README.zh-CN.md#快速开始).

---

## 1. 受支持的平台

`deepseek-tui` 从 v0.8.8 开始为以下平台/架构组合提供预构建二进制：

| 平台 | 架构 | npm 安装 | `cargo install` | GitHub release 资产 |
| ------------ | ------------ | :----: | :-----------: | ----------------------------------------------------- |
| Linux | x64 (x86_64) | ✅ | ✅ | `deepseek-linux-x64`、`deepseek-tui-linux-x64` |
| Linux | arm64 | ✅ | ✅ | `deepseek-linux-arm64`、`deepseek-tui-linux-arm64` |
| macOS | x64 | ✅ | ✅ | `deepseek-macos-x64`、`deepseek-tui-macos-x64` |
| macOS | arm64（M 系列） | ✅ | ✅ | `deepseek-macos-arm64`、`deepseek-tui-macos-arm64` |
| Windows | x64 | ✅ | ✅ | `deepseek-windows-x64.exe`、`deepseek-tui-windows-x64.exe` |
| 其他 Linux（musl、riscv64 等） | — | ❌¹ | ✅² | 从源码构建 |
| FreeBSD / OpenBSD | — | ❌ | ✅² | 从源码构建 |

¹ npm 包会以明确的错误退出并将你指向本页面。
² 前提是你的工具链可以编译较新的 Rust workspace；参见下方的[从源码构建](#5-从源码构建)。

> **Linux ARM64 说明（v0.8.7 及更早版本）。** v0.8.7 及更早版本**不**发布 Linux ARM64 预构建二进制；HarmonyOS 轻薄本、Asahi Linux、树莓派、AWS Graviton 等用户在使用 `npm i -g deepseek-tui` 时会遇到 `Unsupported architecture: arm64`。从 v0.8.8 起同时发布 `deepseek-linux-arm64` 和 `deepseek-tui-linux-arm64`，因此在任何基于 glibc 的 ARM64 Linux 上直接 `npm i -g deepseek-tui` 即可。如果你仍在使用 v0.8.7，请跳转到[从源码构建](#5-从源码构建) —— `cargo install` 可以正常工作。

---

## 2. 通过 npm 安装（推荐）

```bash
npm install -g deepseek-tui
deepseek
```

`postinstall` 从匹配的 GitHub release 下载正确的二进制对，验证 SHA-256 清单，并将 `deepseek` 和 `deepseek-tui` 都暴露在你的 `PATH` 上。

有用的环境变量：

| 变量 | 用途 |
| ----------------------------------- | -------------------------------------------------------------------------------------- |
| `DEEPSEEK_TUI_VERSION` | 指定下载器拉取的发布版本（默认为 `deepseekBinaryVersion`） |
| `DEEPSEEK_TUI_GITHUB_REPO` | 将下载器指向 fork 仓库（`owner/repo`） |
| `DEEPSEEK_TUI_RELEASE_BASE_URL` | 覆盖下载根路径（例如内部镜像或 release-asset 代理） |
| `DEEPSEEK_TUI_FORCE_DOWNLOAD=1` | 即使缓存的二进制标记匹配也重新下载 |
| `DEEPSEEK_TUI_DISABLE_INSTALL=1` | 完全跳过 `postinstall` 下载（CI 冒烟测试、已 vendored 的二进制） |
| `DEEPSEEK_TUI_OPTIONAL_INSTALL=1` | 下载/解压错误时不使 `npm install` 失败 —— 在 CI 矩阵中很有用 |

> **从中国大陆下载 npm 较慢？** 如果 `npm install` 本身（不仅仅是 postinstall 二进制下载）较慢，可以使用 npm 注册表镜像：
> ```bash
> npm config set registry https://registry.npmmirror.com
> npm install -g deepseek-tui
> ```
> 如果你更喜欢 Cargo 而非 npm，另请参阅[第三节](#3-通过-cargo-安装任意-tier-1-rust-目标).

---

## 3. 通过 Cargo 安装（任意 Tier-1 Rust 目标）

如果 GitHub releases 较慢、被屏蔽，或者你使用的是不受支持的架构，可以直接从 crates.io 安装。两个 crate 都是必需的 —— 调度器在运行时会委托给 TUI 运行时。

```bash
# 需要 Rust 1.88+ (https://rustup.rs)
cargo install deepseek-tui-cli --locked   # 提供 `deepseek`
cargo install deepseek-tui     --locked   # 提供 `deepseek-tui`
deepseek --version
```

### 中国大陆 / 镜像友好安装

从中国大陆安装时，同时为 **rustup**（Rust 工具链安装器）和 **Cargo**（包注册表）配置镜像，以避免 TLS 超时和下载失败。

**第一步：通过 rustup 镜像安装 Rust**

```bash
# PowerShell
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
(New-Object Net.WebClient).DownloadFile('https://win.rustup.rs/x86_64', 'rustup-init.exe')

# git-bash / msys2
export RUSTUP_DIST_SERVER=https://mirrors.tuna.tsinghua.edu.cn/rustup
export RUSTUP_UPDATE_ROOT=https://mirrors.tuna.tsinghua.edu.cn/rustup/rustup
./rustup-init.exe -y --default-toolchain stable

# Linux / macOS
export RUSTUP_DIST_SERVER=https://mirrors.tuna.tsinghua.edu.cn/rustup
export RUSTUP_UPDATE_ROOT=https://mirrors.tuna.tsinghua.edu.cn/rustup/rustup
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
```

`RUSTUP_DIST_SERVER` 和 `RUSTUP_UPDATE_ROOT` 环境变量必须在运行 rustup-init **之前**设置；否则工具链下载会遇到和安装器一样的 TLS 握手问题。

**第二步：配置 Cargo 注册表镜像**

```toml
# ~/.cargo/config.toml
[source.crates-io]
replace-with = "tuna"

[source.tuna]
registry = "sparse+https://mirrors.tuna.tsinghua.edu.cn/crates.io-index/"
```

`rsproxy`、腾讯 COS 和阿里云 OSS 镜像的使用方法相同；从你的网络中选择最快的即可。

---

## 4. 从 GitHub Releases 手动下载

从 [Releases 页面](https://github.com/Hmbown/DeepSeek-TUI/releases) 获取与你平台匹配的二进制对，并将它们并排放入 `PATH` 目录（例如 `~/.local/bin`）：

```bash
# Linux ARM64 示例
mkdir -p ~/.local/bin
curl -L -o ~/.local/bin/deepseek      \
    https://github.com/Hmbown/DeepSeek-TUI/releases/latest/download/deepseek-linux-arm64
curl -L -o ~/.local/bin/deepseek-tui  \
    https://github.com/Hmbown/DeepSeek-TUI/releases/latest/download/deepseek-tui-linux-arm64
chmod +x ~/.local/bin/deepseek ~/.local/bin/deepseek-tui
deepseek --version
```

根据每个 release 的 SHA-256 清单校验完整性：

```bash
curl -L -o /tmp/deepseek-artifacts-sha256.txt \
    https://github.com/Hmbown/DeepSeek-TUI/releases/latest/download/deepseek-artifacts-sha256.txt
( cd ~/.local/bin && sha256sum -c /tmp/deepseek-artifacts-sha256.txt --ignore-missing )
```

（macOS 上使用 `shasum -a 256 -c` 代替 `sha256sum`。）

---

## 5. 从源码构建

这是针对我们未提供预构建二进制平台的全能方案 —— 包括 musl、riscv64、LoongArch、FreeBSD 和 2024 年前的 ARM64 发行版。

### 前提条件

- **Rust** 1.88 或更高版本 —— 使用 [rustup](https://rustup.rs) 安装。
- **Linux 构建时依赖**（Debian/Ubuntu/openEuler/Kylin）：
  ```bash
  sudo apt-get install -y build-essential pkg-config libdbus-1-dev
  # openEuler / RHEL 系列：
  # sudo dnf install -y gcc make pkgconf-pkg-config dbus-devel
  ```
- **不需要**安装 `cmake`。

### 构建和安装

```bash
git clone https://github.com/Hmbown/DeepSeek-TUI.git
cd DeepSeek-TUI

cargo install --path crates/cli --locked   # 提供 `deepseek`
cargo install --path crates/tui --locked   # 提供 `deepseek-tui`

deepseek --version
```

两个二进制默认安装到 `~/.cargo/bin/` 目录；请确保该目录在你的 `PATH` 上。

### 从 x64 交叉编译到 ARM64 Linux

如果你想在 x64 Linux 主机上构建 ARM64 Linux 二进制（例如为 HarmonyOS / openEuler ARM64 轻薄本构建），可以使用 [`cross`](https://github.com/cross-rs/cross)，它在 Docker 容器中封装了官方的 Rust 交叉编译目标：

```bash
# 一次性准备
rustup target add aarch64-unknown-linux-gnu
cargo install cross --locked

# 每次构建
cross build --release --target aarch64-unknown-linux-gnu -p deepseek-tui-cli
cross build --release --target aarch64-unknown-linux-gnu -p deepseek-tui
```

生成的二进制文件位于
`target/aarch64-unknown-linux-gnu/release/deepseek` 和
`target/aarch64-unknown-linux-gnu/release/deepseek-tui`。将匹配的二进制对复制到 ARM64 主机（例如通过 `scp`）并 `chmod +x` 即可。

如果你没有 Docker，可以直接安装交叉链接器，让 Cargo 完成工作：

```bash
sudo apt-get install -y gcc-aarch64-linux-gnu
rustup target add aarch64-unknown-linux-gnu

cat >> ~/.cargo/config.toml <<'EOF'
[target.aarch64-unknown-linux-gnu]
linker = "aarch64-linux-gnu-gcc"
EOF

cargo build --release --target aarch64-unknown-linux-gnu -p deepseek-tui-cli
cargo build --release --target aarch64-unknown-linux-gnu -p deepseek-tui
```

同样的方法也适用于 `aarch64-unknown-linux-musl`（如果你的发行版基于 musl）。

### Windows 从源码构建

在 Windows 上构建需要 **MSVC C 工具链**，来自
[Visual Studio Build Tools](https://visualstudio.microsoft.com/downloads/#build-tools-for-visual-studio-2022)
（免费的工作负载可选安装器，而非完整 IDE）。

**前提条件（Windows）**

1. 安装 Visual Studio 2022 Build Tools —— 选择 **"使用 C++ 的桌面开发"** 工作负载。
2. 安装 [Rust](https://rustup.rs) 1.88+（如果从中国大陆下载，参见上文的[中国镜像说明](#中国大陆--镜像友好安装)）。
3. 安装 [Git for Windows](https://git-scm.com/download/win)（提供 `git` 和 `git-bash` 终端）。

**推荐的终端**：Windows Terminal、`git-bash` 或 PowerShell。`cmd.exe` 也可以，但缓冲区较小且 PATH 行为受限。

**设置 MSVC 环境**

Visual Studio Build Tools 将 `cl.exe` 安装到带版本的目录中，但**不会**将其添加到全局 `PATH`。你必须手动设置环境变量，或使用 Developer Command Prompt。所需变量如下：

```powershell
# 将版本号替换为你的安装版本
$msvc = "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Tools\MSVC\14.44.35207"
$sdk   = "C:\Program Files (x86)\Windows Kits\10"
$sdkv  = "10.0.26100.0"

$env:INCLUDE  = "$msvc\include;$msvc\atlmfc\include;$sdk\Include\$sdkv\ucrt;$sdk\Include\$sdkv\um;$sdk\Include\$sdkv\shared"
$env:LIB      = "$msvc\lib\x64;$msvc\atlmfc\lib\x64;$sdk\Lib\$sdkv\ucrt\x64;$sdk\Lib\$sdkv\um\x64"
$env:LIBPATH  = "$msvc\lib\x64;$msvc\atlmfc\lib\x64"
$env:CC       = "$msvc\bin\Hostx64\x64\cl.exe"
$env:CXX      = "$msvc\bin\Hostx64\x64\cl.exe"
$env:PATH     = "$msvc\bin\Hostx64\x64;$env:PATH"
```

或者，打开 **"Developer Command Prompt for VS 2022"**（安装 Build Tools 后从开始菜单可用），它会运行 `vcvars64.bat` 自动完成上述所有配置。然后在该会话中将 `cargo` 添加到 `PATH`，从项目根目录运行 `cargo build`。

**Cargo 注册表镜像** —— 在 Windows 上镜像配置写入 `%USERPROFILE%\.cargo\config.toml`。参见上文的[第二步](#中国大陆--镜像友好安装)。

**构建**

```bash
git clone https://github.com/Hmbown/DeepSeek-TUI.git
cd DeepSeek-TUI
set CARGO_HTTP_CHECK_REVOKE=false   # 在某些中国 ISP 后面可能需要
cargo build --release
```

两个二进制文件出现在 `target\release\deepseek.exe` 和 `target\release\deepseek-tui.exe`。

> **如果不需要修改源码，建议在 Windows 上优先使用 `npm install -g`。**
> npm 包下载预构建二进制，完全避免了对 C 工具链的依赖 —— 参见[第二节](#2-通过-npm-安装推荐).

---

## 6. 故障排除

### `Unsupported architecture: arm64 on platform linux`

你使用的是早于 v0.8.8 的版本，未发布 Linux ARM64 二进制。升级（`npm i -g deepseek-tui@latest`）或按[第三节](#3-通过-cargo-安装任意-tier-1-rust-目标)使用 `cargo install`。

### 运行时出现 `MISSING_COMPANION_BINARY`

调度器（`deepseek`）要求 TUI 运行时（`deepseek-tui`）必须在同一个 `PATH` 上。如果你通过 `cargo install` 只安装了一个 crate，请同时安装两个：

```bash
cargo install deepseek-tui-cli --locked
cargo install deepseek-tui     --locked
```

### `deepseek update` 提示 `no asset found for platform deepseek-linux-aarch64`

这是 v0.8.7 中的 [#503](https://github.com/Hmbown/DeepSeek-TUI/issues/503) —— 自更新器使用了 Rust 的 `aarch64`/`x86_64` 架构名称，而非 release 工件的 `arm64`/`x64`。在更新到 v0.8.8 之前的临时解决方案：

```bash
npm i -g deepseek-tui@latest
# 或者
cargo install deepseek-tui-cli --locked
```

### 从中国大陆下载 npm 较慢或超时

设置 `DEEPSEEK_TUI_RELEASE_BASE_URL` 指向镜像的 release-asset 目录（rsproxy、TUNA、腾讯 COS、阿里云 OSS），或完全跳过 npm，使用[第三节](#3-通过-cargo-安装任意-tier-1-rust-目标)中的 Cargo 镜像设置。

### Debian/Ubuntu：构建时出现 `error: linker 'cc' not found`

安装 C 工具链：

```bash
sudo apt-get install -y build-essential pkg-config libdbus-1-dev
```

### 包装器安装成功但找不到 `deepseek` 命令

`npm i -g` 安装到 `$(npm prefix -g)/bin`；确保该目录在你的 shell `PATH` 中。使用 nvm：`nvm use --lts && hash -r`。

### Windows：`TLS handshake eof` 或 `CRYPT_E_REVOCATION_OFFLINE` 来自 `rustup-init`

从 GFW 后面或某些中国 ISP 访问 `static.rust-lang.org` 的 TLS 握手失败。在运行安装器**之前**设置 rustup 镜像环境变量：

```bash
# git-bash / msys2
export RUSTUP_DIST_SERVER=https://mirrors.tuna.tsinghua.edu.cn/rustup
export RUSTUP_UPDATE_ROOT=https://mirrors.tuna.tsinghua.edu.cn/rustup/rustup
./rustup-init.exe -y --default-toolchain stable
```

如果安装 Rust 后从 Cargo 看到 `CRYPT_E_REVOCATION_OFFLINE`，在 `cargo build` 时同时设置 `CARGO_HTTP_CHECK_REVOKE=false`。

### Windows：`cargo build` 时找不到 MSVC 编译器（`cl.exe`）

Visual Studio Build Tools 不会将 `cl.exe` 添加到全局 `PATH`。可以：

1. 从开始菜单打开 **"Developer Command Prompt for VS 2022"**，在该窗口中添加 `%USERPROFILE%\.cargo\bin` 到 `PATH`，然后从中运行 `cargo build`；或
2. 手动设置 MSVC 环境变量 —— 参见上文的 [Windows 从源码构建](#windows-从源码构建) 部分了解 PowerShell 片段。

验证编译器可达：`cl.exe /?` 应打印帮助文本。

### Windows：Cargo 执行构建脚本时出现 `拒绝访问 (os error 5)`

第三方杀毒软件（火绒、360 安全卫士、卡巴斯基等）可能会阻止 Cargo 执行刚刚编译的构建脚本二进制（例如 `libsqlite3-sys`、`aws-lc-sys`、`instability`）。该错误与路径无关 —— 移动 `target-dir` 不能解决。

**症状**：`could not execute process ... build-script-build (never executed)`

**解决方案**（任选其一）：

1. **将项目的 `target/` 目录添加到杀毒软件排除列表。**
2. **在 `cargo build` 期间临时关闭杀毒软件。**
3. **改用 `npm install -g deepseek-tui`** —— npm 包提供预构建二进制，完全跳过 Cargo 构建（[第二节](#2-通过-npm-安装推荐)）。
4. **使用 `cargo install deepseek-tui-cli --locked`** 从 crates.io 安装 —— 这改变了二进制路径，某些杀毒软件可能区别对待。

要验证构建脚本二进制本身是否有效（未损坏），找到 `target/debug/build/<crate>/build-script-build` 下的文件并手动运行：

```bash
target/debug/build/libsqlite3-sys-*/build-script-build
# 如果能运行但因 "NotPresent"（没有 C 编译器）而 panic，二进制是好的 —— 杀毒软件专门阻止了 Cargo 的进程生成路径。
```

---

## 7. 验证安装

```bash
deepseek --version
deepseek doctor       # 检查 API 密钥、提供商、运行时和 PATH 完整性
deepseek doctor --json
```

`doctor` 如果发现问题会以非零退出，并打印结构化的修复提示。如需帮助，将 JSON 输出粘贴到 GitHub issue 中。
