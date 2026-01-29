"""Commander CLI entry point.

This module provides the main entry point for the Commander CLI.
It re-exports the main function from the chat CLI module.
"""

from commander.chat.cli import main

__all__ = ["main"]

if __name__ == "__main__":
    main()
