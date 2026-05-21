# Docker

[English](DOCKER.md)

DeepSeek-TUI 為每個版本發佈多架構 Linux 映像檔到 GitHub Container Registry。

```bash
docker pull ghcr.io/hmbown/deepseek-tui:latest
```

## 快速開始

使用 Docker 管理的資料卷冊執行已發佈的映像檔：

```bash
docker volume create deepseek-tui-home

docker run --rm -it \
  -e DEEPSEEK_API_KEY="$DEEPSEEK_API_KEY" \
  -v deepseek-tui-home:/home/deepseek/.deepseek \
  -v "$PWD:/workspace" \
  -w /workspace \
  ghcr.io/hmbown/deepseek-tui:latest
```

使用固定的版本標籤以實現可重現的安裝：

```bash
docker run --rm -it \
  -e DEEPSEEK_API_KEY="$DEEPSEEK_API_KEY" \
  -v deepseek-tui-home:/home/deepseek/.deepseek \
  -v "$PWD:/workspace" \
  -w /workspace \
  ghcr.io/hmbown/deepseek-tui:vX.Y.Z
```

將 `vX.Y.Z` 替換為來自 [GitHub Releases](https://github.com/Hmbown/DeepSeek-TUI/releases) 的標籤。

## 本地建構

從檢出的程式碼在本地建構映像檔：

```bash
docker build -t deepseek-tui .
```

然後使用相同的 Docker 管理的資料卷冊來執行：

```bash
docker run --rm -it \
  -e DEEPSEEK_API_KEY="$DEEPSEEK_API_KEY" \
  -v deepseek-tui-home:/home/deepseek/.deepseek \
  -v "$PWD:/workspace" \
  -w /workspace \
  deepseek-tui
```

未設定 Docker Hub 發佈；GHCR 是支援的預建映像檔倉庫。

## 環境變數

| 變數              | 必要 | 說明                                      |
|-----------------------|----------|--------------------------------------------------|
| `DEEPSEEK_API_KEY`    | 是      | DeepSeek API 金鑰                                 |
| `DEEPSEEK_BASE_URL`   | 否       | 自訂 API 基礎 URL（例如 `https://api.deepseek.com`） |
| `DEEPSEEK_NO_COLOR`   | 否       | 設為 `1` 以停用終端機色彩輸出     |

## 卷冊

掛載 `/home/deepseek/.deepseek` 來在工作階段、設定、skills、記憶體和離線佇列在容器重啟之間持久化。Docker 管理的命名卷冊是最安全的預設選擇，因為 Docker 會以容器可以寫入的擁有權來建立它：

```bash
-v deepseek-tui-home:/home/deepseek/.deepseek
```

如果沒有此掛載，容器每次啟動都是全新的。

如果您改為繫結掛載現有的主機目錄，映像檔會以非 root 的 `deepseek` 使用者執行，UID/GID 為 `1000:1000`。掛載的目錄必須可由該使用者寫入，否則在 `.deepseek/tasks` 下建立執行時期目錄時啟動可能會失敗。在 Linux 主機上，請使用上述的命名卷冊，或明確準備繫結掛載：

```bash
mkdir -p ~/.deepseek
sudo chown -R 1000:1000 ~/.deepseek

docker run --rm -it \
  -e DEEPSEEK_API_KEY="$DEEPSEEK_API_KEY" \
  -v ~/.deepseek:/home/deepseek/.deepseek \
  ghcr.io/hmbown/deepseek-tui:latest
```

該 `chown` 會更改主機 `~/.deepseek` 目錄的擁有權。如果您不希望容器 UID 擁有您的本地設定，請跳過此步驟，改用命名卷冊。

## 非互動式 / 管線使用

當 stdin 不是 TTY 時，`deepseek` 會退回到發派器的單次模式（`deepseek -c "…"`）。透過 stdin 傳遞提示：

```bash
echo "Explain the Cargo.toml in structured English." | \
  docker run --rm -i -e DEEPSEEK_API_KEY ghcr.io/hmbown/deepseek-tui:latest
```

## 本地建構

```bash
# 單一平台（您的主機架構）
docker build -t deepseek-tui .

# 多平台（需要具備模擬功能的 builder）
docker buildx create --use
docker buildx build --platform linux/amd64,linux/arm64 -t deepseek-tui .
```

## Devcontainer

此倉庫包含一個 [`.devcontainer/devcontainer.json`](../.devcontainer/devcontainer.json) 設定檔，用於 VS Code / GitHub Codespaces。它預先安裝了 Rust 工具鏈、rust-analyzer 和 `deepseek` 二進位檔。在 devcontainer 中開啟此倉庫即可獲得即開即用的開發環境。

## 發佈狀態

Docker 映像檔發佈是發佈審查的一部分。映像檔發佈至 GHCR，支援 `linux/amd64` 和 `linux/arm64`，並附帶語意化版本標籤及 `latest`。
