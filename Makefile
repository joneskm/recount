# Generate docs and open in browser
doc:
	cargo doc --no-deps --open

# Remove generated docs
clean-doc:
	rm -rf target/doc

.PHONY: doc clean-doc
