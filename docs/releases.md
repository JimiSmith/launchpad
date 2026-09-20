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

## Cutting a release

First commit and push the intended release source **including the workflow**.
Tag the exact reviewed commit; uncommitted files are never included:

```sh
git tag -a v0.2 -m "Launchpad 0.2" <reviewed-commit>
git push origin v0.2
```

Keep the crates' Cargo version and the tag in step: `0.2.0` ships as `v0.2`.
Both workspace members carry the same version. Do not move a published tag to a
different commit.

Complete `bash tools/verify_plugin.sh` before tagging. The workflow builds and
packages the plugin; it never runs the live Zellij PTY suite.

Check the Actions run and release assets before announcing a release:

```sh
gh run list --workflow release.yml
gh release view v0.2
```

Download and verify into an empty directory:

```sh
gh release download v0.2 --repo JimiSmith/zellij-launchpad \
  --pattern 'zellij-launchpad.wasm*'
sha256sum --check zellij-launchpad.wasm.sha256
```

Install at a stable local path and point the plugin layout/alias there. Updating
that file rather than changing its URL preserves the URL-scoped recent-history
cache.

## Asset rename in v0.2

v0.1 published `launchpad-plugin.wasm` and `launchpad-plugin.wasm.sha256`. From
v0.2 the artifact is `zellij-launchpad.wasm`, because the plugin crate was
renamed after the product. Zellij scopes the recent-history cache by plugin URL,
so anyone who points their layout at the new filename **starts with empty
recent launches**; the v0.1 history still exists under the old URL's cache.
Keeping the old local filename preserves that history, at the cost of a name
that no longer matches the release asset.
