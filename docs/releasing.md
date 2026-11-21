# Releasing

A release is a GitHub release `vX.Y.Z` with six desktop files and the Android APK attached, named
by a GitHub milestone (`X.Y.Z`; issues that go into the release are put in it).
The version is the workspace `version` in `Cargo.toml` (and `Cargo.lock`). The
built-in updater reads the release's assets, so file names matter.

**Android is the native app** (`apps/android`, Kotlin over the Rust core; see
`docs/ANDROID.md`). It is built and signed on the release machine, since the
signing key is not in CI. iOS is not released: `apple.yml` still builds the ipa
so the shared code keeps compiling for iOS, but never attaches it.

| Asset | Made by |
| --- | --- |
| `morpheus-X.Y.Z-linux-x86_64.zip` | `linux.yml` |
| `morpheus-X.Y.Z-macos-arm64.zip` | `apple.yml` (macOS, arm64) |
| `morpheus-X.Y.Z-macos-arm64.dmg` | `apple.yml` (macOS, arm64; the disk image for people, the updater uses the zip) |
| `morpheus-X.Y.Z-macos-x86_64.zip` | `apple.yml` (macOS, x86_64) |
| `morpheus-X.Y.Z-macos-x86_64.dmg` | `apple.yml` (macOS, x86_64; as above) |
| `morpheus-X.Y.Z-windows-x86_64.zip` | `windows.yml` |
| `morpheus-X.Y.Z-android-arm64.apk` | `scripts/release-android.sh`, by hand (step 6) |

The desktop zips (and dmgs) are each built, run through the update self-test
(`scripts/update-self-test.sh`) and attached by the workflow. A release is
public the moment it is published, and clients that check for updates then
offer it, so it is published only when all seven assets are there.

## Steps

1. **Version and notes.** Bump `version` in the workspace `Cargo.toml` (run a
   `cargo check` so `Cargo.lock` follows), commit it on `main`. Write the
   release notes as `docs/releases/X.Y.Z.md` (what people get, grouped; the
   closed issues of the milestone are the source) and commit them.
   Everything for the release should be on `origin/main`.

2. **Shake out CI first.** The workflows run on a pushed `v*` tag and can be
   run by hand. Before tagging, run each once from the Actions tab or the CLI
   with an empty `tag`, so they only keep their build as a run artifact and
   attach nothing:

   ```sh
   gh workflow run linux.yml   --ref main -f tag=
   gh workflow run apple.yml   --ref main -f tag=
   gh workflow run windows.yml --ref main -f tag=
   gh run list --limit 5          # then: gh run watch <id>
   ```

   Fix what breaks (CI has the only Windows and macOS builds there are) and
   repeat until all three are green. The macOS and Windows builds take a long
   time; run one at a time if you are also testing locally.

3. **Draft the release.** A draft is invisible to everyone but the team and
   does not create the tag, so the tag is pushed by hand next:

   ```sh
   gh release create vX.Y.Z --draft --title "Morpheus X.Y.Z" \
     --notes-file docs/releases/X.Y.Z.md
   ```

   (Leave `--target` off: the tag pushed next is what the release points at.)

4. **Tag.** An annotated tag on the commit to release, pushed to GitHub:

   ```sh
   git tag -a vX.Y.Z -m "Morpheus X.Y.Z" <commit>
   git push origin vX.Y.Z
   ```

   Shortcut: if step 2's hand runs built this exact commit (the version bump
   included), their artifacts are the release files. Download them (`gh run
   download <id> -n windows-x86_64`, `linux-x86_64`, `macos-arm64`,
   `macos-x86_64`), `gh release upload vX.Y.Z <files> --clobber`, and cancel
   the tag's runs instead of building everything twice.

   The push starts `linux.yml`, `apple.yml` and `windows.yml`; each attaches
   its zips (and dmgs) to the draft release named by the tag. To attach
   again after a fix, dispatch the workflow with `-f tag=vX.Y.Z` (it uses
   `--clobber`, replacing the old file). If the tag has to move, delete it
   on GitHub and push it again.

5. **Tag the tests too.** Put the same tag on the matching commit of the
   private `../morpheus-e2e` and push it, so every release can be lined up
   with its tests (`git -C ../morpheus-e2e tag -a vX.Y.Z -m "Morpheus X.Y.Z"
   && git -C ../morpheus-e2e push origin vX.Y.Z`). Push both repos together.

6. **Android APK.** Built and signed locally with the release key
   (`~/.config/morpheus/release.keystore` and `release.keystore.password`;
   never regenerate, print, copy or commit them: Android installs an update
   only over an app signed with the same key, and the applicationId is
   `dev.morpheus.mobile`):

   ```sh
   nix develop .#android -c bash scripts/release-android.sh
   ```

   It builds `assembleRelease` for arm64 (cargo release profile, R8 on, the keep
   rules in `app/proguard-rules.pro`) and writes
   `target/dist/morpheus-X.Y.Z-android-arm64.apk`. The version is the
   workspace's and `versionCode` is `major*10000 + minor*100 + patch`
   (0.2.7 -> 207), which must be above the last release's. Before uploading,
   compare the certificate with the previous release's APK; the SHA-256
   digests must be identical:

   ```sh
   apksigner verify --print-certs target/dist/morpheus-X.Y.Z-android-arm64.apk
   gh release download vPREVIOUS -p '*android*' -D /tmp/prev && apksigner verify --print-certs /tmp/prev/*.apk
   aapt2 dump badging target/dist/morpheus-X.Y.Z-android-arm64.apk | head -1   # versionCode
   ```

   Smoke test the release build on the emulator with `just
   e2e-android-native-release` (#264; it builds an x86_64 twin signed with the
   debug key, the arm64 file is the one that ships). Then
   `gh release upload vX.Y.Z target/dist/morpheus-X.Y.Z-android-arm64.apk`.

7. **Check, then publish.** All seven assets (six desktop files and the APK) must be attached:

   ```sh
   gh release view vX.Y.Z --json assets -q '.assets[].name'
   ```

   Then make it public and the latest (the updater and the website's
   download links follow "latest"):

   ```sh
   gh release edit vX.Y.Z --draft=false --latest
   ```

   Never publish with an asset missing: people on that platform would be
   offered an update with nothing to install. Do not make a release (or run
   this step) as part of unrelated work.

8. **After.** Close the milestone, move any open issues to the next one, and
   delete the draft artifacts you no longer need (`just sweep` keeps `target/`
   small).

## Milestones

GitHub milestones name releases: the milestone `0.2.4` holds the issues
planned for `v0.2.4`, and closing it means the release is out. `gh issue list
--milestone 0.2.4` is the release's to-do list. New issues without a
milestone are the backlog.

## End-to-end tests

The end-to-end tests are not in this repo: they live in the private
`noahpro99/morpheus-e2e`, checked out as `../morpheus-e2e`. Release builds
never need it (the test bridge compiles only with the `e2e` feature). Before a
release, run `just e2e <file>` for the files you care about, or its headless
workflow (Actions > "e2e (headless)" in that repo).

## The macOS keychain (#159)

Morpheus keeps every account's session in **one** keychain item (service
`dev.morpheus.Morpheus`, account `vault`: a JSON map), read once per launch
and cached; it writes only when something changes. Before, each account had its
own item and every look at the accounts read them again, which on a Mac whose
keychain did not trust the program meant dozens of prompts per launch.

- Old per-account items are moved into the vault on first sight and deleted
  (that is up to one prompt per old item, once).
- Denied, cancelled or locked: the keychain is not asked again in that process,
  nor in later launches (`keychain-denied` in the data folder; delete it to
  try again), and the session lives in the encrypted file instead, with a note
  on the sign-in screen. The earlier sessions are in the keychain, out of reach,
  so a sign-in is needed.
- Dev builds on macOS (a debug build, or a binary outside a `.app`) use the file
  and never prompt. `MORPHEUS_SESSION_STORE=keyring` forces the keychain.
- Release builds are ad-hoc signed (no Apple developer account), so the code
  signature differs per build. `package-macos.sh` pins the designated
  requirement to `identifier "dev.morpheus.desktop"` so that the item's
  access list can match the next version. This is not verified on a real Mac
  (CI has no way to update an app between two signed builds against a real
  keychain ACL). If macOS still asks after an update, the user clicks
  "Always Allow" once per update: one prompt, and the answer sticks until the
  next update. A proper fix is a stable signing identity (a Developer ID or
  a self-signed certificate imported into the user's keychain, which an
  ad-hoc download cannot get).
- CI (`apple.yml`, "Keychain access count"): `scripts/keychain-self-test.sh`
  runs the packaged app's `MORPHEUS_KEYCHAIN_SELF_TEST` against a throwaway
  keychain and checks one read and no writes for a launch with 3 accounts.
