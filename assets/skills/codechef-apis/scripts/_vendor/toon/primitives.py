"""Primitive encoding utilities."""

import re
from typing import List, Optional

from .constants import (
    BACKSLASH,
    CARRIAGE_RETURN,
    CLOSE_BRACE,
    CLOSE_BRACKET,
    COLON,
    COMMA,
    DOUBLE_QUOTE,
    FALSE_LITERAL,
    LIST_ITEM_MARKER,
    NEWLINE,
    NULL_LITERAL,
    OPEN_BRACE,
    OPEN_BRACKET,
    TAB,
    TRUE_LITERAL,
)
from .types import Delimiter, JsonPrimitive


def encode_primitive(value: JsonPrimitive, delimiter: str = COMMA) -> str:
    """Encode a primitive value.

    Args:
        value: Primitive value
        delimiter: Current delimiter being used

    Returns:
        Encoded string
    """
    if value is None:
        return NULL_LITERAL
    if isinstance(value, bool):
        return TRUE_LITERAL if value else FALSE_LITERAL
    if isinstance(value, (int, float)):
        return str(value)
    if isinstance(value, str):
        return encode_string_literal(value, delimiter)
    return str(value)


def escape_string(value: str) -> str:
    """Escape special characters in a string.

    Args:
        value: String to escape

    Returns:
        Escaped string
    """
    result = value
    result = result.replace(BACKSLASH, BACKSLASH + BACKSLASH)
    result = result.replace(DOUBLE_QUOTE, BACKSLASH + DOUBLE_QUOTE)
    result = result.replace(NEWLINE, BACKSLASH + "n")
    result = result.replace(CARRIAGE_RETURN, BACKSLASH + "r")
    result = result.replace(TAB, BACKSLASH + "t")
    return result


def is_safe_unquoted(value: str, delimiter: str = COMMA) -> bool:
    """Check if a string can be safely unquoted.

    Args:
        value: String to check
        delimiter: Current delimiter being used

    Returns:
        True if string doesn't need quotes
    """
    if not value:
        return False

    # Check for leading/trailing whitespace
    if value != value.strip():
        return False

    # Check for reserved literals
    if value in (NULL_LITERAL, TRUE_LITERAL, FALSE_LITERAL):
        return False

    # Check if it looks like a number
    try:
        float(value)
        return False
    except ValueError:
        pass

    # Check if starts with list marker (hyphen)
    if value.startswith(LIST_ITEM_MARKER):
        return False

    # Check for structural characters (including current delimiter)
    unsafe_chars = [
        COLON,
        delimiter,  # Current delimiter
        OPEN_BRACKET,
        CLOSE_BRACKET,
        OPEN_BRACE,
        CLOSE_BRACE,
        DOUBLE_QUOTE,
        BACKSLASH,
        NEWLINE,
        CARRIAGE_RETURN,
        TAB,
    ]

    if any(char in value for char in unsafe_chars):
        return False

    return True


def encode_string_literal(value: str, delimiter: str = COMMA) -> str:
    """Encode a string, quoting only if necessary.

    Args:
        value: String value
        delimiter: Current delimiter being used

    Returns:
        Encoded string
    """
    if is_safe_unquoted(value, delimiter):
        return value
    return f'{DOUBLE_QUOTE}{escape_string(value)}{DOUBLE_QUOTE}'


def encode_key(key: str) -> str:
    """Encode an object key.

    Args:
        key: Key string

    Returns:
        Encoded key
    """
    # Keys matching /^[A-Z_][\w.]*$/i don't require quotes
    if re.match(r"^[A-Z_][\w.]*$", key, re.IGNORECASE):
        return key
    return f'{DOUBLE_QUOTE}{escape_string(key)}{DOUBLE_QUOTE}'


def join_encoded_values(values: List[str], delimiter: Delimiter) -> str:
    """Join encoded primitive values with a delimiter.

    Args:
        values: List of encoded values
        delimiter: Delimiter to use

    Returns:
        Joined string
    """
    return delimiter.join(values)


def format_header(
    key: Optional[str],
    length: int,
    fields: Optional[List[str]],
    delimiter: Delimiter,
    length_marker: Optional[str],
) -> str:
    """Format array/table header.

    Args:
        key: Optional key name
        length: Array length
        fields: Optional field names for tabular format
        delimiter: Delimiter character
        length_marker: Optional length marker prefix

    Returns:
        Formatted header string
    """
    # Build length marker
    marker_prefix = length_marker if length_marker else ""

    # Build fields if provided
    fields_str = ""
    if fields:
        fields_str = f"{OPEN_BRACE}{delimiter.join(fields)}{CLOSE_BRACE}"

    # Build length string with delimiter when needed
    # Rules:
    # - WITH fields: always include delimiter in bracket: [N,] or [N|] or [N\t]
    # - WITHOUT fields: only include if delimiter is not comma: [N] vs [N|]
    if fields:
        # Tabular format: always show delimiter after length
        length_str = f"{OPEN_BRACKET}{marker_prefix}{length}{delimiter}{CLOSE_BRACKET}"
    elif delimiter != COMMA:
        # Primitive array with non-comma delimiter: show delimiter
        length_str = f"{OPEN_BRACKET}{marker_prefix}{length}{delimiter}{CLOSE_BRACKET}"
    else:
        # Primitive array with comma delimiter: just [length]
        length_str = f"{OPEN_BRACKET}{marker_prefix}{length}{CLOSE_BRACKET}"

    # Combine parts
    if key:
        return f"{encode_key(key)}{length_str}{fields_str}{COLON}"
    return f"{length_str}{fields_str}{COLON}"
