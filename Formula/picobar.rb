# Built from source, so macOS never quarantines the binary (no Gatekeeper prompt).
# The release workflow updates url and sha256 on each tag.
class Picobar < Formula
  desc "Menu bar monitor for Claude and OpenCode usage, limit resets and agents"
  homepage "https://github.com/lido772/picobar"
  url "https://github.com/lido772/picobar/archive/refs/tags/v0.3.0.tar.gz"
  sha256 "8579a603ab3ee8f889e784dec6393b81263101021899eaf7b68b60fc4cabe8d4"
  license "MIT"
  head "https://github.com/lido772/picobar.git", branch: "main"

  depends_on "rust" => :build
  depends_on :macos

  def install
    system "cargo", "install", *std_cargo_args
  end

  def caveats
    <<~EOS
      Start PicoBar with:
        picobar &
      Then use Settings > Launch at login in its menu.
      Update with `brew upgrade picobar` (the built-in updater is off for Homebrew installs).
    EOS
  end

  test do
    assert_predicate bin/"picobar", :executable?
  end
end
