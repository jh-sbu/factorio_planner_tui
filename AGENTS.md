# Project Guidance

This project is a Rust-based TUI for planning Factorio factories. Use Ratatui
for the terminal user interface and prefer Rust crates, tooling, and
implementations throughout the application.

Keep the factory-planning domain logic independent from terminal rendering and
input handling so that calculations can be tested without running the TUI.

Development must follow test-driven development:

1. Write a failing test that describes the desired behavior or reproduces the
   bug.
2. Implement the smallest change needed to make the test pass.
3. Refactor while keeping the test suite green.

Add or update tests for every behavior change and bug fix. Run `cargo test`
before considering work complete.
