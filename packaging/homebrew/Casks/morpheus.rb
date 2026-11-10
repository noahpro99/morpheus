# For the personal tap noahpro99/homebrew-tap (Casks/morpheus.rb):
#   brew install --cask noahpro99/tap/morpheus
# The app is not notarized, so homebrew/cask would not take it; in a tap
# the quarantine flag is cleared after installing, so it opens without the
# "cannot be opened" dialog.
cask "morpheus" do
  arch arm: "arm64", intel: "x86_64"

  version "0.1.3"
  sha256 arm:   "c90df36d4a74ccecb85df35d3fce91e2120f917e130fb759726e4e1f2e344088",
         intel: "de9277b833b36f1996e764a35d6dc3d61d4b82db8cdc5736869ee6f332794790"

  url "https://github.com/noahpro99/morpheus/releases/download/v#{version}/morpheus-#{version}-macos-#{arch}.zip",
      verified: "github.com/noahpro99/morpheus/"
  name "Morpheus"
  desc "Discord-style Matrix client with voice channels and calls"
  homepage "https://morpheuschat.org/"

  livecheck do
    url :url
    strategy :github_latest
  end

  # Settings → Update installs new releases in place.
  auto_updates true
  depends_on macos: ">= :catalina"

  app "Morpheus.app"

  postflight do
    system_command "/usr/bin/xattr",
                   args: ["-dr", "com.apple.quarantine", "#{appdir}/Morpheus.app"]
  end

  zap trash: [
    "~/Library/Application Support/dev.morpheus.Morpheus",
    "~/Library/Caches/dev.morpheus.Morpheus",
    "~/Library/Preferences/dev.morpheus.desktop.plist",
    "~/Library/Saved Application State/dev.morpheus.desktop.savedState",
    "~/Library/WebKit/dev.morpheus.desktop",
  ]
end
