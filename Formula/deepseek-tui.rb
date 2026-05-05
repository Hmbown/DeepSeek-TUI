class DeepseekTui < Formula
  desc "Coding agent for DeepSeek models that runs in your terminal"
  homepage "https://github.com/Hmbown/DeepSeek-TUI"
  version "0.8.12"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/Hmbown/DeepSeek-TUI/releases/download/v0.8.12/deepseek-macos-arm64"
      sha256 "573b11d2da67927f29faf518baa772347f07c291f6cefd68d5bef550d5ce794c"

      resource "deepseek-tui-bin" do
        url "https://github.com/Hmbown/DeepSeek-TUI/releases/download/v0.8.12/deepseek-tui-macos-arm64"
        sha256 "8cfa9085807be9a3808fe23db56321f620291ab0ea22a004c31aae0da678438b"
      end
    end

    on_intel do
      url "https://github.com/Hmbown/DeepSeek-TUI/releases/download/v0.8.12/deepseek-macos-x64"
      sha256 "d865fe32d44a8c067d10b23046b02e8fc7cb0943c2bf7e84a2b433c226e496ed"

      resource "deepseek-tui-bin" do
        url "https://github.com/Hmbown/DeepSeek-TUI/releases/download/v0.8.12/deepseek-tui-macos-x64"
        sha256 "eaf03d8d96ce9454147bd7e651b731592957babe376c4404d820c6da380ed8f8"
      end
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/Hmbown/DeepSeek-TUI/releases/download/v0.8.12/deepseek-linux-arm64"
      sha256 "a97d2b292788b862d2b56b8c8c44385d196cc18048ce13690d97d4f5fa190ac6"

      resource "deepseek-tui-bin" do
        url "https://github.com/Hmbown/DeepSeek-TUI/releases/download/v0.8.12/deepseek-tui-linux-arm64"
        sha256 "9fa08253673f675a59882f2dc1a10582fb9da91616cd664382843de244fd0bb4"
      end
    end

    on_intel do
      url "https://github.com/Hmbown/DeepSeek-TUI/releases/download/v0.8.12/deepseek-linux-x64"
      sha256 "052236ba569e908c516a760502c0b94da4bb22a61a31df9a6624ee9dd3b96a78"

      resource "deepseek-tui-bin" do
        url "https://github.com/Hmbown/DeepSeek-TUI/releases/download/v0.8.12/deepseek-tui-linux-x64"
        sha256 "77e4e991d80aab09244a726232fb84145b81cb3a28c588d2a55a2a0807277861"
      end
    end
  end

  def install
    bin.install Dir["*"].first => "deepseek"

    resource("deepseek-tui-bin").stage do
      bin.install Dir["*"].first => "deepseek-tui"
    end
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/deepseek --version")
    assert_match version.to_s, shell_output("#{bin}/deepseek-tui --version")
  end
end
