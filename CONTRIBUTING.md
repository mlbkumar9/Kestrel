# Contributing Guidelines

Thank you for considering contributing to our project! We welcome contributions from everyone. Here are some guidelines to help you get started:

## Reporting Bugs
If you encounter a bug, please open an issue in the repository and provide as much detail as possible, including steps to reproduce the bug.

## Suggesting Features
We love new ideas! If you have a feature suggestion, please open an issue and describe your idea.

## Code Style Guidelines
We follow [PEP 8](https://www.python.org/dev/peps/pep-0008/) for Python code. Please ensure your code adheres to these guidelines.

## Pull Request Process
1. Fork the repository.
2. Create a new branch for your changes.
3. Make your changes and commit them.
4. Open a pull request and describe your changes.

## Development Setup Instructions
For the native Rust application, install Rust 1.92 or later and run these commands
from the repository root:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build -p kestrel-media --release
cargo run -p kestrel-media --release
```

The workspace contains `crates/kestrel-app` (the `kestrel-media` package and
`kestrel` binary) and `crates/kestrel-core`. Build output goes in root `target/`.
FFmpeg integration tests require a modern FFmpeg and FFprobe on `PATH`.

For the legacy Python applications:

1. Clone the repository.
2. Install dependencies using `pip install -r requirements.txt`.
3. Run tests using `pytest`.

## Testing Guidelines
Please ensure that you add tests for any new features or bug fixes.

## Commit Message Conventions
Use clear and descriptive commit messages. Follow the format: `Type: Subject` where Type can be feat, fix, docs, style, refactor, perf, test.

## Code of Conduct
Please adhere to our [Code of Conduct](CODE_OF_CONDUCT.md) in all interactions.