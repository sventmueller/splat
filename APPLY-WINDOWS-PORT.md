# Splat Windows port

The Windows desktop port is isolated under `windows/` and does not replace the existing native macOS implementation.

## Working branch

```powershell
git switch windows-port
```

## Build on Windows 10 / 11 (x64)

Install the repository-pinned Rust toolchain, Microsoft C++ Build Tools (Desktop development with C++), and WebView2 Runtime. Then run:

```powershell
cargo install tauri-cli --version '^2' --locked
cd windows/src-tauri
cargo tauri dev
cargo tauri build --bundles nsis
```

The NSIS installer appears in `windows/src-tauri/target/release/bundle/nsis/`.

## Automated builds and releases

`.github/workflows/windows-release.yml` builds on pushes to `windows-port` and `main`, pull requests touching the Windows port, or manual dispatch. Successful builds are uploaded as the `Splat-Windows-x64` Actions artifact.

A release is deliberately tag-driven after the Windows build has been reviewed:

```powershell
git tag windows-v0.1.0
git push origin windows-v0.1.0
```

The tag workflow builds the installer first, then publishes a GitHub Release with the executable attached. Early builds are unsigned; SmartScreen can identify them as an unknown publisher. Do not disable Defender or SmartScreen merely to install an unreviewed binary.

No personal access token is needed by the workflow. The release job uses GitHub Actions' short-lived `GITHUB_TOKEN` with `contents: write`, limited to that job.
