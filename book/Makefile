CHAPTERS=$(wildcard src/*.md)
ENTRY=tests/src/lib.rs

test: $(CHAPTERS) $(ENTRY)
	cd tests && cargo test

$(ENTRY): $(CHAPTERS)
	mkdir -p tests/src
	find tests/src -maxdepth 1 -type f -delete
	echo '#![allow(nonstandard_style)]' >> $(ENTRY)
	for chapter in $(CHAPTERS); do \
		name="$${chapter%.*}"; \
		echo "#![cfg_attr(doctest, doc = include_str!(\"../../$$chapter\"))]" > "tests/$$name.rs"; \
		echo "mod $$(basename $$name);" >> $(ENTRY); \
	done

.PHONY: test
