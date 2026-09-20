# Releases

Pushing a tag beginning with `v` runs `.github/workflows/release.yml`. It builds
only the production plugin for `wasm32-wasip1`, with the toolchain pinned in
`rust-toolchain.toml` and dependencies from `Cargo.lock` (`--locked`). The
development-only worker-fault feature is not included.

The workflow publishes a GitHub release containing:

- `zellij-launchpad.wasm`
- `zellij-launchpad.wasm.sha256`

New releases remain drafts until both assets upload. Rerunning a failed workflow
can finish the draft; assets for the same tag are replaced. Replacing assets on
an already-published release is not atomic: a failed rerun can temporarily leave
missing or mismatched assets. Rerun successfully before using those downloads.
Tags containing a hyphen (for example `v0.2-rc1`) are marked as prereleases. The repository's
visibility is unchanged; private-repository assets require authentication.

## First release: v0.1

First commit and push the intended release source **including the workflow**.
Tag the exact reviewed commit; uncommitted files are never included:

```sh
git tag -a v0.1 -m "Launchpad 0.1" <reviewed-commit>
git push origin v0.1
```

Cargo's package version is `0.1.0`; the release/tag name is `v0.1`. Later releases
use another `v` tag. Do not move a published tag to a different commit.

Check the Actions run and release assets before announcing a release:

```sh
gh run list --workflow release.yml
gh release view v0.1
```

Download and verify into an empty directory:

```sh
gh release download v0.1 --repo JimiSmith/zellij-launchpad \
  --pattern 'zellij-launchpad.wasm*'
sha256sum --check zellij-launchpad.wasm.sha256
```

Install at a stable local path and point the plugin layout/alias there. Updating
that file rather than changing its URL preserves the URL-scoped recent-history
cache. The workflow builds/packages the plugin; it does not run the live Zellij
PTY verification suite, which should be completed before tagging.
