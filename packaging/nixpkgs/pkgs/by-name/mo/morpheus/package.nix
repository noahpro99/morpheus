{
  lib,
  rustPlatform,
  fetchFromGitHub,
  fetchzip,
  dioxus-cli,
  pkg-config,
  cmake,
  llvmPackages_21,
  makeWrapper,
  wrapGAppsHook3,
  copyDesktopItems,
  makeDesktopItem,
  gst_all_1,
  glib-networking,
  gtk3,
  webkitgtk_4_1,
  libsoup_3,
  glib,
  cairo,
  pango,
  gdk-pixbuf,
  harfbuzz,
  librsvg,
  at-spi2-atk,
  xdotool,
  libayatana-appindicator,
  libxkbcommon,
  wayland,
  libGL,
  vulkan-loader,
  openssl,
  sqlite,
  gsettings-desktop-schemas,
  alsa-lib,
  libpulseaudio,
  pipewire,
  libx11,
  libxext,
  libxfixes,
  libxdamage,
  libxrandr,
  libxcomposite,
  libxtst,
  libdrm,
  libgbm,
}:

let
  clang = llvmPackages_21.clang;
  gstPlugins = with gst_all_1; [
    gstreamer
    gst-plugins-base
    gst-plugins-good
  ];
  # Linked, or loaded at run time: the tray, sound, screen capture (X11), GL.
  libs = [
    gtk3
    webkitgtk_4_1
    libsoup_3
    glib
    cairo
    pango
    gdk-pixbuf
    harfbuzz
    librsvg
    at-spi2-atk
    xdotool
    libayatana-appindicator
    libxkbcommon
    wayland
    libGL
    vulkan-loader
    openssl
    sqlite
    gsettings-desktop-schemas
    alsa-lib
    libpulseaudio
    pipewire
    libx11
    libxext
    libxfixes
    libxdamage
    libxrandr
    libxcomposite
    libxtst
    libdrm
    libgbm
  ];
in
rustPlatform.buildRustPackage (finalAttrs: {
  pname = "morpheus";
  version = "0.1.3";

  src = fetchFromGitHub {
    owner = "noahpro99";
    repo = "morpheus";
    tag = "v${finalAttrs.version}";
    hash = "sha256-g8x8YlsPmxG6mGnc+xAMo9FcNPSRkzfh7x5jGHUALrs=";
  };

  cargoHash = "sha256-pi33SWdLzCYRnXV55lLbSSq7c4t+RLZ26jGcXK4Mw4I=";

  # LiveKit's prebuilt libwebrtc (calls), which webrtc-sys downloads while
  # building otherwise; the tag is webrtc-sys-build's WEBRTC_TAG.
  env = {
    LK_CUSTOM_WEBRTC = fetchzip {
      url = "https://github.com/livekit/rust-sdks/releases/download/webrtc-89d790b/webrtc-linux-x64-release.zip";
      hash = "sha256-qsjhLygyCqIJrk0PMxoP2aoUdd3DPvtFtYrlIOBLpx0=";
    };
    # libwebrtc is built against Chromium's libc++: a recent clang for C++.
    CC_x86_64_unknown_linux_gnu = "${clang}/bin/clang";
    CXX_x86_64_unknown_linux_gnu = "${clang}/bin/clang++";
  };

  nativeBuildInputs = [
    dioxus-cli
    pkg-config
    cmake
    clang
    makeWrapper
    wrapGAppsHook3
    copyDesktopItems
  ];
  buildInputs = libs ++ gstPlugins ++ [ glib-networking ];
  dontUseCmakeConfigure = true;

  # dx bundles the app's assets beside the binary, where it looks for them.
  buildPhase = ''
    runHook preBuild
    export HOME=$TMPDIR
    dx build --release --package morpheus-desktop --platform linux
    runHook postBuild
  '';
  doCheck = false;

  installPhase = ''
    runHook preInstall
    app=target/dx/morpheus-desktop/release/linux/app
    mkdir -p $out/lib/morpheus $out/bin
    cp -r $app/morpheus-desktop $app/assets $out/lib/morpheus/
    install -Dm644 apps/desktop/icons/icon.svg $out/share/icons/hicolor/scalable/apps/morpheus.svg
    runHook postInstall
  '';

  desktopItems = [
    (makeDesktopItem {
      name = "morpheus";
      desktopName = "Morpheus";
      genericName = "Matrix client";
      comment = "Chat, voice and video on Matrix";
      exec = "morpheus";
      icon = "morpheus";
      categories = [
        "Network"
        "InstantMessaging"
        "Chat"
      ];
      startupWMClass = "morpheus-desktop";
    })
  ];

  dontWrapGApps = true;
  postFixup = ''
    makeWrapper $out/lib/morpheus/morpheus-desktop $out/bin/morpheus \
      "''${gappsWrapperArgs[@]}" \
      --set WEBKIT_DISABLE_DMABUF_RENDERER 1 \
      --prefix GST_PLUGIN_SYSTEM_PATH_1_0 : "${lib.makeSearchPathOutput "lib" "lib/gstreamer-1.0" gstPlugins}" \
      --prefix LD_LIBRARY_PATH : "${lib.makeLibraryPath libs}"
  '';

  meta = {
    description = "Discord-style Matrix client with voice channels and calls";
    homepage = "https://morpheuschat.org";
    changelog = "https://github.com/noahpro99/morpheus/releases/tag/v${finalAttrs.version}";
    license = with lib.licenses; [
      mit
      asl20
    ];
    sourceProvenance = with lib.sourceTypes; [
      fromSource
      binaryNativeCode # LiveKit's prebuilt libwebrtc
    ];
    maintainers = [ ];
    mainProgram = "morpheus";
    platforms = [ "x86_64-linux" ];
  };
})
