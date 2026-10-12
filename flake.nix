{
  description = "Morpheus - a Discord-style Matrix client in Rust (Dioxus)";

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
    in
    {
      formatter = forEachSystem (pkgs: pkgs.nixfmt-rfc-style);

      devShells = forEachSystem (
        pkgs:
        let
          toolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;

          # Native libraries the Dioxus desktop (webview) build links against on Linux.
          guiLibs = with pkgs; [
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
            libxkbcommon
            wayland
            libGL
            vulkan-loader
            openssl
            sqlite
            gsettings-desktop-schemas
          ];

          tools = with pkgs; [
            dioxus-cli
            just
            pkg-config
            cmake # aws-lc-sys (rustls backend used by matrix-sdk)
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
            MORPHEUS_HOMESERVER = "http://localhost:8008";
          };

          shellHook = ''
            export XDG_DATA_DIRS="$GSETTINGS_SCHEMAS_PATH''${XDG_DATA_DIRS:+:$XDG_DATA_DIRS}"
            echo "morpheus dev shell — run 'just' to list commands"
          '';
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

          # `nix develop .#android` — adds the Android SDK/NDK and Rust targets for `dx serve --platform android`.
          android =
            let
              androidToolchain = toolchain.override {
                targets = [
                  "aarch64-linux-android"
                  "x86_64-linux-android"
                ];
              };
              androidSdk =
                (pkgs.androidenv.composeAndroidPackages {
                  platformVersions = [ "35" ];
                  buildToolsVersions = [ "35.0.0" ];
                  includeNDK = true;
                  includeEmulator = false;
                  includeSystemImages = false;
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
                '';
              }
            );
        }
      );
    };
}
