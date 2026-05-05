.PHONY: check fmt format lint run test

fmt:
	cargo fmt

format: fmt

lint:
	cargo fmt --check
	cargo check

test:
	cargo test

check: lint test

run:
	cargo run
