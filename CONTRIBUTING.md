# Contributing to CC View

Thanks for your interest in contributing! CC View is a macOS menubar app built with Tauri 2 (Rust) + Vue 3 + TypeScript.

## Prerequisites

- **macOS 13+** (the app uses macOS-specific APIs)
- **Rust** (stable toolchain) — [rustup](https://rustup.rs/)
- **Node.js 20+** — [nodejs.org](https://nodejs.org/)
- **Xcode Command Line Tools** — `xcode-select --install`

## Development Setup

```bash
git clone https://github.com/Ethanzyc/cc-view.git
cd cc-view
npm install
npm run tauri dev
```

The app launches in the menubar. Press `⌥Space` to open the command palette.

## Project Structure

```
src-tauri/src/   Rust backend (collector, reducer, notify, focus, prefs)
src/             Vue 3 frontend (components, composables, i18n)
src/types.ts     Shared types (aligned with Rust serde)
```

## Code Style

- **Rust**: Run `cargo fmt` before committing (CI enforces this).
- **TypeScript/Vue**: `vue-tsc --noEmit` must pass (part of `npm run build`).
- **Comments**: Chinese for internal comments, English for public-facing code.
- Keep it simple — readability first.

## Before Submitting a PR

1. **Format**: `cd src-tauri && cargo fmt`
2. **Lint**: `cd src-tauri && cargo clippy --all-targets`
3. **Test**: `cd src-tauri && cargo test --lib`
4. **Build**: `npm run build` (includes vue-tsc type check)
5. **Commit message**: Follow [Conventional Commits](https://www.conventionalcommits.org/) (e.g., `feat:`, `fix:`, `docs:`, `refactor:`).

## Pull Request Process

1. Fork the repo and create a feature branch from `main`.
2. Make your changes, keeping commits focused.
3. Ensure CI passes (fmt + clippy + test + build).
4. Open a PR with a clear description of what and why.

## Reporting Bugs

Use [GitHub Issues](https://github.com/Ethanzyc/cc-view/issues). Include:

- macOS version and CPU architecture (Apple Silicon / Intel)
- CC View version (menubar → Preferences → check version)
- Steps to reproduce
- Expected vs actual behavior

## License

By contributing, you agree that your contributions are licensed under the [MIT License](LICENSE).
