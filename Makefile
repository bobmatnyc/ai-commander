# AI Commander — project shortcuts
# Usage: make <target>

SERVICES    := ~/.ai-commander/services.sh
DAEMON      := ~/.ai-commander/bin/commander-daemon
BOLD        := \033[1m
GREEN       := \033[0;32m
BLUE        := \033[0;34m
YELLOW      := \033[1;33m
NC          := \033[0m

.PHONY: build start restart stop status logs connect pair help

## build: compile release binaries and install to ~/.ai-commander/bin/
build:
	@./scripts/services.sh install

## start: start daemon + telegram bot (no-op if already running)
start:
	@$(SERVICES) start

## restart: restart daemon + telegram bot
restart:
	@$(SERVICES) restart

## stop: stop both services
stop:
	@$(SERVICES) stop

## status: show running status and recent logs
status:
	@$(SERVICES) status

## logs: tail live logs from both services
logs:
	@$(SERVICES) logs

## pair: generate a one-time pairing code for a new Telegram device
pair:
	@if [ ! -x "$(DAEMON)" ]; then \
		echo "$(DAEMON) not found — run 'make build' first"; exit 1; \
	fi
	@printf "\n$(BOLD)Pairing a Telegram account$(NC)\n\n"
	@printf "1. Run this to get your code:\n\n"
	@printf "   $(BLUE)$(DAEMON) pair$(NC)\n\n"
	@$(DAEMON) pair 2>&1 || true
	@printf "\n2. In Telegram, send:\n\n"
	@printf "   $(BLUE)/pair <code>$(NC)\n\n"

## connect: show how to connect Telegram to a project directory
connect:
	@printf "\n$(BOLD)Connecting Telegram to a project$(NC)\n\n"
	@printf "In Telegram, send:\n\n"
	@printf "  $(BLUE)/connect $(CURDIR)$(NC)   ← this project\n\n"
	@printf "Or connect to any path:\n\n"
	@printf "  $(BLUE)/connect /path/to/project$(NC)\n\n"
	@printf "List active sessions:  $(BLUE)/list$(NC)\n"
	@printf "Disconnect:            $(BLUE)/disconnect$(NC)\n\n"

## help: show available targets
help:
	@printf "\n$(BOLD)AI Commander — available targets$(NC)\n\n"
	@grep -E '^## ' Makefile | sed 's/## //' | awk -F': ' \
		'{printf "  $(GREEN)make %-12s$(NC) %s\n", $$1, $$2}'
	@printf "\n"
