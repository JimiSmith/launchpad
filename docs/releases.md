# Native releases

Tags beginning with `v` run the release workflow. It builds the native executable
on Ubuntu 24.04 for Linux x86_64 using the pinned Rust toolchain and locked Cargo
dependencies. The GNU binary requires a compatible glibc (Ubuntu 24.04 or newer).

Assets:

- `zellij-launchpad-linux-x86_64.tar.gz` (executable, documentation, layouts and example config)
- `zellij-launchpad-linux-x86_64.tar.gz.sha256`

New releases remain drafts until both assets upload. Hyphenated tags are marked
prereleases. Reruns replace matching assets; publication is not atomic for an
already published release. No WASM artifact is built or distributed.

Before tagging, run `bash tools/verify_native.sh`. Keep both Cargo package versions
and the release tag aligned. Commit and push the reviewed source before tagging;
do not move published tags. Implementation of this migration does not itself tag,
push, or publish a release.

Extract the archive, verify its checksum, and install the binary on the Zellij
server's PATH. Existing plugin users must update layouts and move configuration
to TOML; native history starts fresh in XDG state storage.
