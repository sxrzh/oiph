"""Type definitions for pytoon."""

from typing import Any, Dict, List, Literal, TypedDict, Union

# JSON-compatible types
JsonPrimitive = Union[str, int, float, bool, None]
JsonObject = Dict[str, Any]
JsonArray = List[Any]
JsonValue = Union[JsonPrimitive, JsonArray, JsonObject]

# Delimiter type
Delimiter = str
DelimiterKey = Literal["comma", "tab", "pipe"]


class EncodeOptions(TypedDict, total=False):
    """Options for TOON encoding.

    Attributes:
        indent: Number of spaces per indentation level (default: 2)
        delimiter: Delimiter character for arrays (default: comma)
        lengthMarker: Optional marker to prefix array lengths (default: False)
    """

    indent: int
    delimiter: Delimiter
    lengthMarker: Literal["#", False]


class ResolvedEncodeOptions:
    """Resolved encoding options with defaults applied."""

    def __init__(
        self,
        indent: int = 2,
        delimiter: str = ",",
        length_marker: Literal["#", False] = False,
    ) -> None:
        self.indent = indent
        self.delimiter = delimiter
        self.lengthMarker = length_marker


class DecodeOptions:
    """Options for TOON decoding.

    Attributes:
        indent: Number of spaces per indentation level (default: 2)
        strict: Enable strict validation (default: True)
    """

    def __init__(self, indent: int = 2, strict: bool = True) -> None:
        self.indent = indent
        self.strict = strict


# Depth type for tracking indentation level
Depth = int
