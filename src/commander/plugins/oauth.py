"""OAuth plugin interface for optional authentication integration.

This module defines the interface for OAuth management that can be
implemented by external packages to provide authentication capabilities.
"""

from typing import Any, Protocol, runtime_checkable


@runtime_checkable
class IOAuthManager(Protocol):
    """Protocol for OAuth authentication management.

    Implement this interface to provide OAuth functionality
    to the commander REPL.
    """

    def get_credentials(self, service: str) -> dict[str, Any] | None:
        """Get stored credentials for a service.

        Args:
            service: The service identifier (e.g., 'google', 'github')

        Returns:
            Credentials dictionary if available, None otherwise
        """
        ...

    def authenticate(self, service: str) -> bool:
        """Initiate authentication flow for a service.

        Args:
            service: The service identifier

        Returns:
            True if authentication was successful
        """
        ...

    def revoke(self, service: str) -> bool:
        """Revoke credentials for a service.

        Args:
            service: The service identifier

        Returns:
            True if revocation was successful
        """
        ...

    def list_authenticated_services(self) -> list[str]:
        """List all services with valid credentials.

        Returns:
            List of service identifiers
        """
        ...


class OAuthManagerStub:
    """Stub implementation when OAuth is not available.

    This stub provides graceful degradation when the OAuth
    integration package is not installed.
    """

    def get_credentials(self, service: str) -> dict[str, Any] | None:
        """Return None - no credentials available."""
        return None

    def authenticate(self, service: str) -> bool:
        """Return False - authentication not available."""
        return False

    def revoke(self, service: str) -> bool:
        """Return False - revocation not available."""
        return False

    def list_authenticated_services(self) -> list[str]:
        """Return empty list - no services available."""
        return []


# Global registry for OAuth manager implementation
_oauth_manager: IOAuthManager | None = None


def get_oauth_manager() -> IOAuthManager | None:
    """Get the registered OAuth manager, if any.

    Returns:
        The registered OAuth manager or None if not registered
    """
    return _oauth_manager


def register_oauth_manager(manager: IOAuthManager) -> None:
    """Register an OAuth manager implementation.

    Args:
        manager: The OAuth manager to register
    """
    global _oauth_manager
    _oauth_manager = manager
