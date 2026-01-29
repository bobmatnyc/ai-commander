"""Plugin interfaces for optional external dependencies.

This module provides stub interfaces for optional integrations that
can be implemented by external packages (e.g., claude-mpm) to extend
commander functionality.
"""

from commander.plugins.mcp import IMCPServiceRegistry, MCPServiceRegistryStub
from commander.plugins.oauth import IOAuthManager, OAuthManagerStub

__all__ = [
    "IMCPServiceRegistry",
    "MCPServiceRegistryStub",
    "IOAuthManager",
    "OAuthManagerStub",
]
