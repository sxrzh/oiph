"""Line writer for managing indented output."""

from typing import List

from .types import Depth


class LineWriter:
    """Manages indented text output."""

    def __init__(self, indent_size: int) -> None:
        """Initialize the line writer.

        Args:
            indent_size: Number of spaces per indentation level
        """
        self._lines: List[str] = []
        self._indentation_string = " " * indent_size

    def push(self, depth: Depth, content: str) -> None:
        """Add a line with appropriate indentation.

        Args:
            depth: Indentation depth level
            content: Content to add
        """
        indent = self._indentation_string * depth
        self._lines.append(f"{indent}{content}")

    def to_string(self) -> str:
        """Return all lines joined with newlines.

        Returns:
            Complete output string
        """
        return "\n".join(self._lines)
