install:
	bun install

format:
	cargo fmt
	bun prettier --write .

lint:
	cargo clippy

dev:
	cargo watch -L 'debug,axum::rejection=trace' --why -x 'run --bin crabot'
