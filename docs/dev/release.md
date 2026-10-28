# Releases

Run from the git-all checkout, using the next version:

```bash
script/release --dry-run 0.7.8
script/release 0.7.8
```

The script commits the version bump and pushes a tag. GitHub Actions builds
binaries for macOS and Linux (ARM64 and x86_64), publishes the release, then
updates `rsanheim/homebrew-tap`. Each archive includes the binary and man page.

Check the Release workflow and the tap's CI, then verify the installed version:

```bash
brew update
brew upgrade git-all  # or: brew install rsanheim/tap/git-all
git-all meta
```

## GitHub token setup

The release workflow needs permission to push to the separate tap repository.
Create a fine-grained token for `rsanheim/homebrew-tap` with **Contents: Read and
write**, then save it at the prompt:

```bash
gh secret set HOMEBREW_TAP_TOKEN --repo rsanheim/git-all
```

Replace this secret when the token expires.

## Retry a Homebrew update

If the Homebrew job fails, fix the reported error and rerun the failed job.
The GitHub Release stays published; an unchanged formula needs no new commit.

To update Homebrew from an existing release without rebuilding binaries:

```bash
gh workflow run release.yml --ref main -f version=0.7.7
```

For local formula work, use `script/update-homebrew <version>`. Set `TAP_PATH`
if the tap checkout is not at `~/src/rsanheim/homebrew-tap`. The script updates
URLs and hashes; commit and push the formula from that checkout.
