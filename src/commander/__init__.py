"""MPM Commander - Multi-Project Orchestration.

This module provides the core infrastructure for managing multiple projects
with isolated state, work queues, and tool sessions.

Key Components:
    - ProjectRegistry: Thread-safe project management
    - Project models: Data structures for state and sessions
    - TmuxOrchestrator: Tmux session and pane management
    - Config loading: .claude-mpm/ directory configuration
    - CommanderDaemon: Main daemon process for orchestration
    - ProjectSession: Per-project lifecycle management
    - InstanceManager: Framework selection and instance lifecycle
    - Frameworks: Claude Code, MPM framework abstractions
    - Memory: Conversation storage, semantic search, context compression

Example:
    >>> from claude_mpm.commander import ProjectRegistry
    >>> registry = ProjectRegistry()
    >>> project = registry.register("/path/to/project")
    >>> registry.update_state(project.id, ProjectState.WORKING)

    >>> # Memory integration
    >>> from commander.memory import MemoryIntegration
    >>> memory = MemoryIntegration.create()
    >>> await memory.capture_project_conversation(project)
"""

from commander.config import DaemonConfig
from commander.config_loader import load_project_config
from commander.daemon import CommanderDaemon
from commander.frameworks import (
    BaseFramework,
    ClaudeCodeFramework,
    InstanceInfo,
    MPMFramework,
)
from commander.instance_manager import (
    FrameworkNotFoundError,
    InstanceAlreadyExistsError,
    InstanceManager,
    InstanceNotFoundError,
)
from commander.models import (
    Project,
    ProjectState,
    ThreadMessage,
    ToolSession,
)
from commander.project_session import ProjectSession, SessionState
from commander.registry import ProjectRegistry
from commander.tmux_orchestrator import (
    TmuxNotFoundError,
    TmuxOrchestrator,
)

__all__ = [
    "BaseFramework",
    "ClaudeCodeFramework",
    "CommanderDaemon",
    "DaemonConfig",
    "FrameworkNotFoundError",
    "InstanceAlreadyExistsError",
    "InstanceInfo",
    "InstanceManager",
    "InstanceNotFoundError",
    "MPMFramework",
    "Project",
    "ProjectRegistry",
    "ProjectSession",
    "ProjectState",
    "SessionState",
    "ThreadMessage",
    "TmuxNotFoundError",
    "TmuxOrchestrator",
    "ToolSession",
    "load_project_config",
]
