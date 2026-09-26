# Native releases

Tags beginning with `v` run the release workflow. It builds and tests each native
executable on a matching runner using the pinned Rust toolchain and locked Cargo
dependencies:

| Platform | Rust target | Runner | Archive |
| --- | --- | --- | --- |
| Linux x64 | `x86_64-unknown-linux-gnu` | Ubuntu 24.04 | `launchpad-linux-x86_64.tar.gz` |
| Windows x64 | `x86_64-pc-windows-msvc` | Windows 2022 | `launchpad-windows-x86_64.zip` |
| macOS Intel x64 | `x86_64-apple-darwin` | macOS 15 Intel | `launchpad-macos-x86_64.tar.gz` |
| macOS Apple Silicon ARM64 | `aarch64-apple-darwin` | macOS 15 ARM64 | `launchpad-macos-aarch64.tar.gz` |

The Linux GNU binary requires a compatible glibc (Ubuntu 24.04 or newer).

Each archive includes the executable (`launchpad.exe` on Windows), README,
documentation, layouts and example config. Every archive has a matching `.sha256`
checksum file.

Publication starts only after all four builds pass formatting, tests and Clippy,
and all downloaded archives pass checksum verification. New releases remain
drafts until all eight assets upload. Hyphenated tags are marked prereleases.
Reruns replace matching assets; publication is not atomic for an already published
release. No WASM artifact is built or distributed.

Before tagging, run `bash tools/verify_native.sh`. Keep both Cargo package versions,
the `version` in `herdr-plugin.toml` and the release tag aligned: the herdr
plugin's install script downloads the release named by that version, and a test
fails when it differs from the package version. Commit and push the reviewed source before tagging;
do not move published tags. Implementation of this migration does not itself tag,
push, or publish a release.

Verify the archive's checksum, extract it, and install the binary on the Zellij
server's PATH. Existing plugin users must update layouts and move configuration
to TOML; native history starts fresh in XDG state storage.
