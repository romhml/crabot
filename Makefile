format:
	cargo fmt
	bun prettier --write .

lint:
	cargo clippy

dev:
	cargo watch -L debug --why -x 'run --bin crabot'
