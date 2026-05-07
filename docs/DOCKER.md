# Docker

DeepSeek TUI ships an official multi-arch Docker image (amd64 + arm64) on
[GitHub Container Registry](https://github.com/Hmbown/DeepSeek-TUI/pkgs/container/deepseek-tui).

## Quick start

```bash
docker run --rm -it \
  -e DEEPSEEK_API_KEY="$DEEPSEEK_API_KEY" \
  -v ~/.deepseek:/home/deepseek/.deepseek \
  ghcr.io/hmbown/deepseek-tui:latest
```

Images are published to GitHub Container Registry (GHCR) only. Docker Hub
publishing is not currently configured — add a `docker/login-action` step
with Hub credentials to the release workflow if needed.

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

## Availability

The image is built and pushed automatically on every release tag via CI.
If the image is not publicly accessible or the pull fails, verify GHCR
authentication:

```bash
echo "$GITHUB_TOKEN" | docker login ghcr.io -u YOUR_USERNAME --password-stdin
```

See [INSTALL.md](INSTALL.md) for alternative install paths if Docker is unavailable.

## Tags

| Tag        | Meaning                  |
|------------|--------------------------|
| `latest`   | Latest stable release    |
| `v0`       | Latest v0.x release      |
| `0.8.9`    | Specific release version |

Docker images are built and pushed automatically when a release tag is pushed
(see [release.yml](../.github/workflows/release.yml)).
