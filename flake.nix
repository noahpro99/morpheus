{
  description = "Morpheus - a Discord-style Matrix client in Rust (GPUI desktop, Dioxus phone and web)";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    { nixpkgs, rust-overlay, ... }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forEachSystem =
        f:
        nixpkgs.lib.genAttrs systems (
          system:
          f (
            import nixpkgs {
              inherit system;
              overlays = [ rust-overlay.overlays.default ];
              config = {
                allowUnfree = true;
                android_sdk.accept_license = true;
              };
            }
          )
        );

      # Native libraries the Dioxus desktop (webview) build links against on Linux.
      guiLibsFor =
        pkgs: with pkgs; [
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
          xdotool # libxdo, needed by the desktop renderer (muda/tao)
          libayatana-appindicator # system tray icon (tray-icon dlopens it)
          libxkbcommon
          wayland
          libGL
          vulkan-loader
          openssl
          sqlite
          gsettings-desktop-schemas
          alsa-lib # call audio (cpal)
          libpulseaudio
          pipewire
          # libwebrtc (calls) loads these at runtime, for screen capture.
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

      # Native libraries the GPUI desktop app links and loads at run time
      # (Wayland, X11 and Vulkan for the window, fontconfig and freetype for
      # text, sound, and what libwebrtc loads for calls and screen capture).
      desktopLibsFor =
        pkgs: with pkgs; [
          wayland
          libxkbcommon # also libxkbcommon-x11
          libxcb
          libx11
          libxext
          libxfixes
          libxdamage
          libxrandr
          libxcomposite
          libxtst
          libdrm
          libgbm
          libGL
          vulkan-loader
          fontconfig
          freetype
          openssl
          sqlite
          alsa-lib # call audio and sounds (cpal, rodio)
          libpulseaudio
          pipewire
        ];
    in
    {
      formatter = forEachSystem (pkgs: pkgs.nixfmt-rfc-style);

      # `nix build` / `nix run`: the desktop app, for installing on NixOS
      # (github:noahpro99/morpheus). The GPUI app, built with cargo: its fonts and
      # icons are inside the binary.
      packages = nixpkgs.lib.genAttrs [ "x86_64-linux" ] (
        system:
        let
          pkgs = import nixpkgs {
            inherit system;
            overlays = [ rust-overlay.overlays.default ];
          };
          inherit (pkgs) lib;
          toolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
          rustPlatform = pkgs.makeRustPlatform {
            cargo = toolchain;
            rustc = toolchain;
          };
          clang = pkgs.llvmPackages_21.clang;
          desktopLibs = desktopLibsFor pkgs;
          # LiveKit's prebuilt libwebrtc (calls), which webrtc-sys would
          # download while building: the tag is webrtc-sys-build's WEBRTC_TAG.
          webrtc = pkgs.fetchzip {
            url = "https://github.com/livekit/rust-sdks/releases/download/webrtc-89d790b/webrtc-linux-x64-release.zip";
            hash = "sha256-qsjhLygyCqIJrk0PMxoP2aoUdd3DPvtFtYrlIOBLpx0=";
          };
          desktopItem = pkgs.makeDesktopItem {
            name = "morpheus";
            desktopName = "Morpheus";
            genericName = "Matrix client";
            comment = "Chat, voice and video on Matrix";
            exec = "morpheus %u";
            icon = "morpheus";
            categories = [
              "Network"
              "InstantMessaging"
              "Chat"
            ];
            # The window's app id, so a launcher or dock matches the window to this entry.
            startupWMClass = "morpheus-desktop";
            # matrix: links open in Morpheus (passed to the running one).
            mimeTypes = [ "x-scheme-handler/matrix" ];
          };
          morpheus = rustPlatform.buildRustPackage {
            pname = "morpheus";
            version = (builtins.fromTOML (builtins.readFile ./Cargo.toml)).workspace.package.version;
            src = lib.fileset.toSource {
              root = ./.;
              fileset = lib.fileset.unions [
                ./Cargo.toml
                ./Cargo.lock
                ./rust-toolchain.toml
                ./apps
                ./crates
                ./vendor # the patched gpui and xattr
              ];
            };
            cargoLock = {
              lockFile = ./Cargo.lock;
              # DeepFilterNet (crates/call), from its repository at the pinned commit.
              outputHashes."deep_filter-0.5.7-pre" = "sha256-27iMSscu9QLrWbhjJfI5+xVWBekFg9UY2SVkmWTzqIQ=";
            };

            nativeBuildInputs = [
              pkgs.pkg-config
              pkgs.cmake # aws-lc-sys
              pkgs.nasm # OpenH264 (videos)
              clang # webrtc-sys runs clang++ itself
              pkgs.makeWrapper
              pkgs.copyDesktopItems
            ];
            buildInputs = desktopLibs;
            cargoBuildFlags = [
              "--package"
              "morpheus-desktop"
            ];

            LK_CUSTOM_WEBRTC = webrtc;
            # As in the dev shell: libwebrtc is built against Chromium's libc++.
            CC_x86_64_unknown_linux_gnu = "${clang}/bin/clang";
            CXX_x86_64_unknown_linux_gnu = "${clang}/bin/clang++";
            dontUseCmakeConfigure = true;

            doCheck = false;
            # cargo's install hook puts the binary in bin/; it moves to lib/ and
            # bin/morpheus becomes the wrapper (postFixup).
            postInstall = ''
              mkdir -p $out/lib/morpheus
              mv $out/bin/morpheus-desktop $out/lib/morpheus/
              install -Dm644 apps/desktop/icons/icon.png $out/share/icons/hicolor/1024x1024/apps/morpheus.png
              install -Dm644 apps/desktop/icons/icon.svg $out/share/icons/hicolor/scalable/apps/morpheus.svg
            '';

            desktopItems = [ desktopItem ];
            # One wrapper, with what is loaded at run time: the window system
            # (Wayland, X11, Vulkan, GL), sound, screen capture.
            postFixup = ''
              makeWrapper $out/lib/morpheus/morpheus-desktop $out/bin/morpheus \
                --prefix LD_LIBRARY_PATH : "${lib.makeLibraryPath desktopLibs}"
            '';

            meta = {
              description = "A Discord-style Matrix client";
              homepage = "https://github.com/noahpro99/morpheus";
              license = with lib.licenses; [
                mit
                asl20
              ];
              mainProgram = "morpheus";
              platforms = [ "x86_64-linux" ];
            };
          };
        in
        {
          inherit morpheus;
          default = morpheus;
        }
      );

      devShells = forEachSystem (
        pkgs:
        let
          toolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;

          guiLibs = guiLibsFor pkgs;

          tools = with pkgs; [
            dioxus-cli
            cargo-sweep # scripts/prune-target.sh
            just
            pkg-config
            cmake # aws-lc-sys (rustls backend used by matrix-sdk)
            nasm # OpenH264's x86 assembly (videos; plain C without it)
            jq
            curl
            docker-compose
            grim # window screenshots for e2e tests (Wayland)
          ];

          commonEnv = {
            LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath guiLibs;
            # TLS for webkitgtk (remote media, avatars).
            GIO_MODULE_DIR = "${pkgs.glib-networking}/lib/gio/modules";
            # Without this webkitgtk often renders a blank window on NixOS + Wayland.
            WEBKIT_DISABLE_DMABUF_RENDERER = "1";
            # Sound in the webview (call chimes, later audio and video): webkitgtk
            # plays through GStreamer, which finds its plugins here.
            GST_PLUGIN_SYSTEM_PATH_1_0 =
              with pkgs.gst_all_1;
              pkgs.lib.makeSearchPathOutput "lib" "lib/gstreamer-1.0" [
                gstreamer
                gst-plugins-base
                gst-plugins-good
                gst-libav # H.264 and H.265 (videos in messages)
              ];
            MORPHEUS_HOMESERVER = "http://localhost:8008";
            # Calls: libwebrtc (webrtc-sys) is built against Chromium's libc++ and
            # needs a recent clang for the host; gcc stays the default elsewhere.
            CC_x86_64_unknown_linux_gnu = "${pkgs.llvmPackages_21.clang}/bin/clang";
            CXX_x86_64_unknown_linux_gnu = "${pkgs.llvmPackages_21.clang}/bin/clang++";
          };

          shellHook = ''
            export XDG_DATA_DIRS="$GSETTINGS_SCHEMAS_PATH''${XDG_DATA_DIRS:+:$XDG_DATA_DIRS}"
            # Once a day: keep target/ to the builds in use (it grew past 300 GB).
            stamp=target/.pruned
            if [ -d target ] && [ -z "$(find "$stamp" -mmin -1440 2>/dev/null)" ]; then
              touch "$stamp"
              ./scripts/prune-target.sh >&2 || true
            fi
            echo "morpheus dev shell — run 'just' to list commands"
          '';
          androidShell =
            withEmulator:
            let
              androidToolchain = toolchain.override {
                targets = [
                  "aarch64-linux-android"
                  "x86_64-linux-android"
                ];
              };
              androidSdk =
                (pkgs.androidenv.composeAndroidPackages {
                  # dx 0.7's gradle template compiles against 34; the SDK in the
                  # store is read-only, so gradle cannot fetch what is missing.
                  # System images follow the platform list, so keep it to one.
                  platformVersions = [ "34" ];
                  buildToolsVersions = [
                    "34.0.0"
                    "35.0.0"
                  ];
                  includeNDK = true;
                  includeEmulator = withEmulator;
                  includeSystemImages = withEmulator;
                  systemImageTypes = [ "google_apis" ];
                  abiVersions = [ "x86_64" ];
                }).androidsdk;
              androidHome = "${androidSdk}/libexec/android-sdk";
            in
            pkgs.mkShell (
              commonEnv
              // {
                nativeBuildInputs = tools ++ [
                  androidToolchain
                  androidSdk
                  pkgs.jdk17
                ];
                buildInputs = guiLibs;
                ANDROID_HOME = androidHome;
                ANDROID_SDK_ROOT = androidHome;
                JAVA_HOME = pkgs.jdk17.home;
                # NixOS gotcha: gradle's bundled aapt2 is not patched for the nix store.
                GRADLE_OPTS = "-Dorg.gradle.project.android.aapt2FromMavenOverride=${androidHome}/build-tools/35.0.0/aapt2";
                shellHook = shellHook + ''
                  export ANDROID_NDK_HOME="$(ls -d "$ANDROID_HOME"/ndk/* | head -n1)"
                  export NDK_HOME="$ANDROID_NDK_HOME"
                  # Let plain cargo (and the C deps behind the SDK) use the NDK toolchain too.
                  NDK_BIN="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin"
                  NDK_CLANG="$(ls "$NDK_BIN"/aarch64-linux-android*-clang | sort -V | tail -n1)"
                  export CC_aarch64_linux_android="$NDK_CLANG" CXX_aarch64_linux_android="$NDK_CLANG++"
                  export AR_aarch64_linux_android="$NDK_BIN/llvm-ar" CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$NDK_CLANG"
                  # The emulator is x86_64.
                  NDK_CLANG_X86="$(ls "$NDK_BIN"/x86_64-linux-android*-clang | sort -V | tail -n1)"
                  export CC_x86_64_linux_android="$NDK_CLANG_X86" CXX_x86_64_linux_android="$NDK_CLANG_X86++"
                  export AR_x86_64_linux_android="$NDK_BIN/llvm-ar" CARGO_TARGET_X86_64_LINUX_ANDROID_LINKER="$NDK_CLANG_X86"
                '';
              }
            );
        in
        {
          default = pkgs.mkShell (
            commonEnv
            // {
              nativeBuildInputs = tools ++ [ toolchain ];
              buildInputs = guiLibs;
              inherit shellHook;
            }
          );
          # `nix develop .#web` — the browser build (app.morpheuschat.org):
          # the wasm target, wasm-bindgen as Cargo.lock has it (dx checks the
          # versions match; nixpkgs' dx brings an older one, last in PATH),
          # and a headless Chromium the e2e tests drive.
          web =
            let
              wasmBindgen = pkgs.buildWasmBindgenCli rec {
                src = pkgs.fetchCrate {
                  pname = "wasm-bindgen-cli";
                  version = "0.2.129";
                  hash = "sha256-pcecKQd7E8Opw6bkFoE569epUi7gh5qpQF1e5PJY6V8=";
                };
                cargoDeps = pkgs.rustPlatform.fetchCargoVendor {
                  inherit src;
                  inherit (src) pname version;
                  hash = "sha256-vmUrWVU7kPJJxO5qIVeAkwQyWDELO1Z4Z5gitz2kco8=";
                };
              };
            in
            pkgs.mkShell (
              commonEnv
              // {
                nativeBuildInputs = tools ++ [
                  (toolchain.override { targets = [ "wasm32-unknown-unknown" ]; })
                  wasmBindgen
                  pkgs.binaryen
                  pkgs.chromium
                  pkgs.llvmPackages_21.clang-unwrapped
                ];
                buildInputs = guiLibs;
                # C in the SDK's dependencies (ring, zstd) compiled for wasm.
                CC_wasm32_unknown_unknown = "${pkgs.llvmPackages_21.clang-unwrapped}/bin/clang";
                AR_wasm32_unknown_unknown = "${pkgs.llvmPackages_21.llvm}/bin/llvm-ar";
                CHROME = "${pkgs.chromium}/bin/chromium";
                inherit shellHook;
              }
            );
          # `nix develop .#android` — the Android SDK/NDK and Rust targets for
          # `dx serve --platform android` on a plugged-in phone.
          android = androidShell false;
          # `nix develop .#android-emulator` — the same plus the emulator and an
          # x86_64 system image (a few GB) for the virtual phone.
          android-emulator = androidShell true;
          # `nix develop .#windows` — cross-builds the desktop app for Windows
          # (MSVC, as LiveKit's prebuilt WebRTC is), through cargo-xwin:
          # Microsoft's headers and libraries are downloaded on first use.
          windows = pkgs.mkShell {
            nativeBuildInputs = [
              (toolchain.override { targets = [ "x86_64-pc-windows-msvc" ]; })
              pkgs.cargo-xwin
              pkgs.llvmPackages_21.clang-unwrapped
              pkgs.llvmPackages_21.lld
              pkgs.llvmPackages_21.llvm
              pkgs.cmake
              pkgs.nasm
              pkgs.dioxus-cli
              pkgs.just
              pkgs.jq
              pkgs.zip
            ];
          };
        }
      );
    };
}
