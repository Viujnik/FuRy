SHELL       := /bin/bash
.DEFAULT_GOAL := bench

COMPOSE     := docker compose -f docker-compose.yml
PSQL        := $(COMPOSE) exec -T postgres psql -U fury -d fury_bench
STAMP       := $(shell date -u +%Y%m%dT%H%M%SZ)
RESULTS     := results/$(STAMP)

.PHONY: all db seed bench python node rust report clean help

all: bench

help:
	@echo "targets: db seed bench python node rust report clean"

db:
	$(COMPOSE) up -d --wait

seed: db
	@echo "→ seeding (this drops and recreates the users table)"
	@$(PSQL) -q < seed.sql

bench: seed
	@mkdir -p $(RESULTS)
	@bash scripts/run_all.sh $(STAMP)
	@ln -sfn $(STAMP) results/latest
	@echo
	@echo "✓ report: $(RESULTS)/REPORT.md"

python: seed
	@bash scripts/run_all.sh $(STAMP) --only python
	@ln -sfn $(STAMP) results/latest

node: seed
	@bash scripts/run_all.sh $(STAMP) --only node
	@ln -sfn $(STAMP) results/latest

rust:
	@bash scripts/run_all.sh $(STAMP) --only rust
	@ln -sfn $(STAMP) results/latest

report:
	@python3 scripts/report.py results/$(shell readlink results/latest 2>/dev/null || echo .)

clean:
	$(COMPOSE) down -v || true
	rm -rf results/2* results/latest
	rm -rf fixtures/*
	rm -rf node/node_modules rust/target
