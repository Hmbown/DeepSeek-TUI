# Docker

DeepSeek-TUI publishes a multi-arch Linux image to GitHub Container Registry
for each release.

```bash
docker pull ghcr.io/hmbown/deepseek-tui:latest
```

## Quick start

Run the published image with your existing config directory mounted:

```bash
docker run --rm -it \
  -e DEEPSEEK_API_KEY="$DEEPSEEK_API_KEY" \
  -v ~/.deepseek:/home/deepseek/.deepseek \
  ghcr.io/hmbown/deepseek-tui:latest
```

Use a pinned release tag for reproducible installs:

```bash
docker run --rm -it \
  -e DEEPSEEK_API_KEY="$DEEPSEEK_API_KEY" \
  -v ~/.deepseek:/home/deepseek/.deepseek \
  ghcr.io/hmbown/deepseek-tui:v0.8.20
```

## Local build

Build the image locally from a checkout:

```bash
docker build -t deepseek-tui .
```

Then run it with your existing config directory mounted:

```bash
docker run --rm -it \
  -e DEEPSEEK_API_KEY="$DEEPSEEK_API_KEY" \
  -v ~/.deepseek:/home/deepseek/.deepseek \
  deepseek-tui
```

Docker Hub publishing is not configured; GHCR is the supported prebuilt image
registry.

## Environment variables

| Variable              | Required | Description                                      |
|-----------------------|----------|--------------------------------------------------|
| `DEEPSEEK_API_KEY`    | yes      | DeepSeek API key                                 |
| `DEEPSEEK_BASE_URL`   | no       | Custom API base URL (e.g. `https://api.deepseek.com`) |
| `DEEPSEEK_NO_COLOR`   | no       | Set to `1` to disable terminal colour output     |

## Volumes

Mount `~/.deepseek` to persist sessions, config, skills, memory, and the offline queue
across container restarts:

```bash
-v ~/.deepseek:/home/deepseek/.deepseek
```

Without this mount the container starts fresh each time.

## Non-interactive / pipeline usage

When stdin is not a TTY, `deepseek` drops to the dispatcher's one-shot mode
(`deepseek -c "…"`). Pipe a prompt on stdin:

```bash
echo "Explain the Cargo.toml in structured English." | \
  docker run --rm -i -e DEEPSEEK_API_KEY ghcr.io/hmbown/deepseek-tui:latest
```

## Building locally

```bash
# Single platform (your host architecture)
docker build -t deepseek-tui .

# Multi-platform (requires a builder with emulation)
docker buildx create --use
docker buildx build --platform linux/amd64,linux/arm64 -t deepseek-tui .
```

## Devcontainer

The repository includes a [`.devcontainer/devcontainer.json`](../.devcontainer/devcontainer.json)
configuration for VS Code / GitHub Codespaces. It pre-installs the Rust toolchain,
rust-analyzer, and the `deepseek` binary. Open the repo in a devcontainer to get a
ready-to-use development environment.

## Release status

Docker image publishing is part of the release gate. The image is published to
GHCR for `linux/amd64` and `linux/arm64` with semver tags plus `latest`.
