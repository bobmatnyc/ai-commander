# Commander

CLI framework orchestration and AI-powered command execution.

> **Note**: This project was extracted from [claude-mpm](https://github.com/masa/claude-mpm) to serve as a standalone CLI framework.

## Installation

```bash
# Basic installation
pip install ai-commander

# With OAuth support
pip install ai-commander[oauth]

# Development installation
pip install -e ".[dev]"
```

## Usage

```bash
# Run commander
commander --help
```

## Development

### Setup

```bash
# Clone the repository
git clone https://github.com/masa/commander.git
cd commander

# Create virtual environment
python -m venv .venv
source .venv/bin/activate  # On Windows: .venv\Scripts\activate

# Install with dev dependencies
pip install -e ".[dev]"

# Install pre-commit hooks
pre-commit install
```

### Running Tests

```bash
# Run tests with coverage
pytest

# Run specific test file
pytest tests/test_example.py -v
```

### Code Quality

```bash
# Run linter
ruff check src/ tests/

# Run formatter
ruff format src/ tests/

# Run type checker
mypy src/
```

## License

MIT License - see [LICENSE](LICENSE) for details.
