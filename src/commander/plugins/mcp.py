"""MCP (Model Context Protocol) plugin interface for optional integration.

This module defines the interface for MCP service registry that can be
implemented by external packages to provide MCP server management.
"""

from typing import Any, Protocol, runtime_checkable


@runtime_checkable
class IMCPServiceRegistry(Protocol):
    """Protocol for MCP service registry management.

    Implement this interface to provide MCP server functionality
    to the commander REPL.
    """

    def get_service(self, name: str) -> Any | None:
        """Get an MCP service by name.

        Args:
            name: The service name

        Returns:
            The service instance if found, None otherwise
        """
        ...

    def list_services(self) -> list[str]:
        """List all registered MCP services.

        Returns:
            List of service names
        """
        ...

    def register_service(self, name: str, service: Any) -> None:
        """Register an MCP service.

        Args:
            name: The service name
            service: The service instance
        """
        ...

    def unregister_service(self, name: str) -> bool:
        """Unregister an MCP service.

        Args:
            name: The service name

        Returns:
            True if the service was unregistered
        """
        ...


class MCPServiceRegistryStub:
    """Stub implementation when MCP is not available.

    This stub provides graceful degradation when the MCP
    integration package is not installed.
    """

    def get_service(self, name: str) -> Any | None:
        """Return None - no services available."""
        return None

    def list_services(self) -> list[str]:
        """Return empty list - no services registered."""
        return []

    def register_service(self, name: str, service: Any) -> None:
        """No-op - service registration not available."""
        pass

    def unregister_service(self, name: str) -> bool:
        """Return False - unregistration not available."""
        return False


# Global registry for MCP service registry implementation
_mcp_registry: IMCPServiceRegistry | None = None


def get_mcp_registry() -> IMCPServiceRegistry | None:
    """Get the registered MCP service registry, if any.

    Returns:
        The registered MCP registry or None if not registered
    """
    return _mcp_registry


def register_mcp_registry(registry: IMCPServiceRegistry) -> None:
    """Register an MCP service registry implementation.

    Args:
        registry: The MCP registry to register
    """
    global _mcp_registry
    _mcp_registry = registry
