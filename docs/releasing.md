# Releasing

A release is a GitHub release `vX.Y.Z` with six desktop files attached, named
by a GitHub milestone (`X.Y.Z`; issues that go into the release are put in it).
The version is the workspace `version` in `Cargo.toml` (and `Cargo.lock`). The
built-in updater reads the release's assets, so file names matter.

**No phone apps in releases** (from 0.2.4): Android and iOS are not released
until they are rebuilt natively (Swift/Kotlin over UniFFI, see
`docs/PLAN.md`). `apple.yml` still builds the iOS ipa so the shared code keeps
compiling for iOS, but never attaches it; the APK is not built for releases.

| Asset | Made by |
| --- | --- |
| `morpheus-X.Y.Z-linux-x86_64.zip` | `linux.yml` |
| `morpheus-X.Y.Z-macos-arm64.zip` | `apple.yml` (macOS, arm64) |
| `morpheus-X.Y.Z-macos-arm64.dmg` | `apple.yml` (macOS, arm64; the disk image for people, the updater uses the zip) |
| `morpheus-X.Y.Z-macos-x86_64.zip` | `apple.yml` (macOS, x86_64) |
| `morpheus-X.Y.Z-macos-x86_64.dmg` | `apple.yml` (macOS, x86_64; as above) |
| `morpheus-X.Y.Z-windows-x86_64.zip` | `windows.yml` |

The desktop zips (and dmgs) are each built, run through the update self-test
(`scripts/update-self-test.sh`) and attached by the workflow. A release is
public the moment it is published, and clients that check for updates then
offer it, so it is published only when all six assets are there.

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

   The push starts `linux.yml`, `apple.yml` and `windows.yml`; each attaches
   its zips (and dmgs) to the draft release named by the tag. To attach
   again after a fix, dispatch the workflow with `-f tag=vX.Y.Z` (it uses
   `--clobber`, replacing the old file). If the tag has to move, delete it
   on GitHub and push it again.

5. **Tag the tests too.** Put the same tag on the matching commit of the
   private `../morpheus-e2e` and push it, so every release can be lined up
   with its tests (`git -C ../morpheus-e2e tag -a vX.Y.Z -m "Morpheus X.Y.Z"
   && git -C ../morpheus-e2e push origin vX.Y.Z`). Push both repos together.

   (Android, for when phones are released again: `nix develop .#android -c
   bash scripts/release-android.sh`, then `gh release upload`; keep
   `~/.config/morpheus/release.keystore`, Android only updates over an app
   signed with the same key.)

6. **Check, then publish.** All six assets must be attached:

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

7. **After.** Close the milestone, move any open issues to the next one, and
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
