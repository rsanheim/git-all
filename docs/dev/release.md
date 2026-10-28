# Release Process

## Overview

git-all is distributed via Homebrew tap using prebuilt binaries. This approach is language-agnostic - the formula downloads architecture-specific binaries regardless of whether they were built with Rust, Zig, or Crystal.

## Architecture

```
rsanheim/git-all                rsanheim/homebrew-tap
├── .github/workflows/          ├── Formula/
│   └── release.yml  ──────────►│   └── git-all.rb
└── script/                     └── .github/workflows/
    ├── release                     └── lint.yml
    └── update-homebrew
```

**Artifact naming** (language-agnostic):
```
git-all-{version}-darwin-arm64.tar.gz
git-all-{version}-darwin-x86_64.tar.gz
git-all-{version}-linux-arm64.tar.gz
git-all-{version}-linux-x86_64.tar.gz
```

## Release Steps

### One-time Authentication Setup

Add an Actions repository secret named `HOMEBREW_TAP_TOKEN` to
`rsanheim/git-all`. Use a fine-grained personal access token restricted to
`rsanheim/homebrew-tap` with **Contents: Read and write** permission. The token
must be allowed to push to the tap's `main` branch.

```bash
gh secret set HOMEBREW_TAP_TOKEN --repo rsanheim/git-all
```

The command prompts for the token. The workflow's built-in `GITHUB_TOKEN`
cannot push to another repository; see
[GitHub's authentication guidance](https://docs.github.com/en/actions/tutorials/authenticate-with-github_token#granting-additional-permissions).
Renew the secret when its token expires.

### 1. Create Release

From the git-all repo:

```bash
# Dry-run first
script/release --dry-run 0.6.0

# Create the release
script/release 0.6.0
```

This will:
* Update version in `rust/Cargo.toml`
* Commit the version bump
* Create and push a `v0.6.0` tag
* GitHub Actions builds binaries and creates a GitHub Release
* After publication, the `Update Homebrew tap` job updates and pushes the formula

### 2. Check the Homebrew Update

The formula uses concrete versioned URLs and lets Homebrew infer the version.
The workflow runs `script/update-homebrew` against a checkout of
`rsanheim/homebrew-tap`, downloads the published artifacts to compute all four
SHA256 hashes, and commits only `Formula/git-all.rb` to the tap's `main` branch.
That push triggers the tap's existing lint and installation tests.

Check that both the release workflow's `Update Homebrew tap` job and the tap's
CI pass. A failed tap update leaves the GitHub Release published but marks the
release workflow as failed. Fix the credential or push error, then rerun the
failed job. An unchanged formula is a successful no-op.

For releases created before this automation, or to update the tap manually:

```bash
# Dry-run to see SHA256 hashes
script/update-homebrew --dry-run 0.6.0

# Update the formula
script/update-homebrew 0.6.0

# Commit and push the tap
cd ~/src/rsanheim/homebrew-tap
git diff  # verify changes
git add Formula/git-all.rb && git commit -m "git-all 0.6.0" && git push
```

### 3. Verify Installation

```bash
brew update
brew upgrade git-all  # or: brew install rsanheim/tap/git-all
git-all meta
```

## Local Testing

Before releasing, test the formula locally:

```bash
cd ~/src/rsanheim/homebrew-tap
brew style Formula/git-all.rb
brew readall rsanheim/tap
brew test rsanheim/tap/git-all

# Optional extra lint pass. Homebrew 6 rejects path-based audit.
brew audit --strict rsanheim/tap/git-all
```

## Changing Implementation Language

When switching from Rust to Zig (or Crystal):

| Component | Changes | No Changes |
|-----------|---------|------------|
| release.yml | `cargo build` → `zig build` | Artifact names, upload step |
| script/release | Cargo.toml → build.zig | Tag/push logic |
| Formula/git-all.rb | None | Downloads same artifacts |

The formula never knows what language built the binary.
