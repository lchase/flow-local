# Homebrew cask for Flow Local.
#
# To use: put this file in a tap repo (e.g. a GitHub repo named
# `homebrew-tools` under your org) at Casks/flow-local.rb, host the dmg
# somewhere URL-accessible (GitHub Releases, S3, internal file server),
# then update `url` and `sha256` below.
#
# Install:  brew tap <org>/tools && brew install --cask --no-quarantine flow-local
# Update:   bump version + sha256, users run `brew upgrade --cask flow-local`
cask "flow-local" do
  version "0.1.0"
  sha256 "REPLACE_WITH_OUTPUT_OF_shasum_-a_256_dmg"

  url "https://example.com/releases/Flow.Local_#{version}_aarch64.dmg"
  name "Flow Local"
  desc "Local-first dictation — on-device speech-to-text with Ollama cleanup"
  homepage "https://example.com/flow-local"

  depends_on arch: :arm64
  depends_on macos: ">= :monterey"

  app "Flow Local.app"

  zap trash: [
    "~/Library/Application Support/com.lawrencechase.flowlocal",
    "~/Library/Preferences/com.lawrencechase.flowlocal.plist",
  ]

  caveats <<~EOS
    Flow Local is not notarized yet — install with --no-quarantine
    (shown above) or right-click → Open on first launch.

    AI transcript cleanup needs Ollama with a small model:
      brew install ollama
      ollama pull qwen3:4b-instruct
    Without it, raw transcripts are still inserted.
  EOS
end
