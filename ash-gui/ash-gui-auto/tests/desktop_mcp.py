"""AutoUI MCP client for ash-gui tests.

Wraps the 12 `autoui_*` MCP tools (HTTP JSON-RPC at localhost:9247) behind a
typed Python API. Used by conftest.py fixtures and individual test files.

Design (per Plan 398 §9.4 research):
- Prefer `vnode_N` ids + `autoui_find`/`exists` for element targeting (robust).
- Tool results are AURA/Atom-formatted plain text (not JSON), so helpers parse
  text. State queries use `autoui_state`.
"""

import re
import time
from dataclasses import dataclass, field
from typing import Optional

import requests

MCP_PORT = int(__import__("os").environ.get("AUTOUI_MCP_PORT", "9247"))
MCP_URL = f"http://127.0.0.1:{MCP_PORT}/mcp"
DEFAULT_TIMEOUT = 10  # seconds per MCP call


class McpError(RuntimeError):
    """Raised when the MCP server returns a JSON-RPC error."""


@dataclass
class FoundNode:
    """A node located via autoui_find/exists."""

    vnode_id: str  # e.g. "vnode_7"
    kind: str  # e.g. "button", "input", "text"
    label: str  # matched label/content
    raw: str = ""  # full text block for debugging


@dataclass
class ActionResult:
    """Parsed result of an autoui_action / autoui_type call."""

    status: str  # "ok" or error
    element: str
    action: str
    handler: Optional[str] = None
    state_changes: list = field(default_factory=list)  # [(field, before, after)]


class McpClient:
    """JSON-RPC client for the AutoUI MCP server (embedded in iced process)."""

    def __init__(self, url: str = MCP_URL):
        self.url = url
        self.req_id = 0

    # ── low-level JSON-RPC ──────────────────────────────────────────────

    def call(self, tool_name: str, **arguments) -> str:
        """Call a tool, return its text result. Raises McpError on RPC error."""
        self.req_id += 1
        resp = requests.post(
            self.url,
            json={
                "jsonrpc": "2.0",
                "method": "tools/call",
                "params": {"name": tool_name, "arguments": arguments},
                "id": self.req_id,
            },
            timeout=DEFAULT_TIMEOUT,
        )
        data = resp.json()
        if "error" in data:
            raise McpError(f"MCP error: {data['error']}")
        content = data.get("result", {}).get("content", [])
        return content[0]["text"] if content else ""

    def tools_list(self) -> list:
        """Return the list of available tool names."""
        self.req_id += 1
        resp = requests.post(
            self.url,
            json={"jsonrpc": "2.0", "method": "tools/list", "params": {}, "id": self.req_id},
            timeout=DEFAULT_TIMEOUT,
        )
        data = resp.json()
        return [t["name"] for t in data.get("result", {}).get("tools", [])]

    # ── high-level helpers ──────────────────────────────────────────────

    def snapshot(self, include_state=True, include_bounds=False) -> str:
        """Full page AURA snapshot. include_state adds the state block."""
        return self.call(
            "autoui_snapshot",
            include_state=include_state,
            include_bounds=include_bounds,
        )

    def vtree(self, depth=10, include_box=True, include_events=True) -> str:
        """Live post-render VTree as Atom text (primary structural channel)."""
        return self.call(
            "autoui_vtree",
            depth=depth,
            include_box=include_box,
            include_events=include_events,
        )

    def state(self, *fields) -> str:
        """Query state values. Empty = all fields. Supports suffix match
        (e.g. 'blocks' matches 'store.blocks')."""
        return self.call("autoui_state", fields=list(fields) if fields else [])

    def find(self, kind: Optional[str] = None, label: Optional[str] = None, limit=20) -> list:
        """Search VTree by kind and/or label (case-insensitive substring).
        Returns list of FoundNode."""
        args = {"limit": limit}
        if kind:
            args["kind"] = kind
        if label:
            args["label"] = label
        text = self.call("autoui_find", **args)
        return _parse_found_nodes(text)

    def exists(self, kind: Optional[str] = None, label: Optional[str] = None) -> bool:
        """Quick existence check. Returns True if at least one match."""
        args = {}
        if kind:
            args["kind"] = kind
        if label:
            args["label"] = label
        text = self.call("autoui_exists", **args)
        return "FOUND" in text and "NOT FOUND" not in text

    def click(self, vnode_id: str) -> ActionResult:
        """Press a button (vnode_N id). Returns parsed action result."""
        text = self.call("autoui_action", element_id=vnode_id, action="press")
        return _parse_action_result(text)

    def click_label(self, kind: str, label: str) -> ActionResult:
        """Find by kind+label and click the first match."""
        nodes = self.find(kind=kind, label=label)
        if not nodes:
            raise McpError(f"No {kind} found with label ~'{label}'")
        return self.click(nodes[0].vnode_id)

    def type_into(self, text: str, element_id: Optional[str] = None, clear_first=True) -> ActionResult:
        """Type text into an input. element_id=None defaults to first input."""
        args = {"text": text, "clear_first": clear_first}
        if element_id:
            args["element_id"] = element_id
        result_text = self.call("autoui_type", **args)
        return _parse_action_result(result_text)

    def key(self, key: str, modifiers: Optional[list] = None) -> str:
        """Send a key / shortcut. key: Enter/Tab/Escape/ArrowUp/...
        modifiers: list of ctrl/shift/alt."""
        args = {"key": key}
        if modifiers:
            args["modifiers"] = modifiers
        return self.call("autoui_keyboard", **args)

    def wait_until(self, condition, timeout=10, interval=0.5) -> bool:
        """Poll a callable returning bool until True or timeout. The callable
        receives this client. Returns True if condition met, False on timeout."""
        deadline = time.time() + timeout
        while time.time() < deadline:
            try:
                if condition(self):
                    return True
            except Exception:
                pass
            time.sleep(interval)
        return False

    def screenshot(self, name: str, baseline=False, diff=False, threshold=0.01) -> str:
        """Capture PNG. baseline=True writes baseline; diff=True compares."""
        return self.call(
            "autoui_screenshot",
            name=name,
            baseline=baseline,
            diff=diff,
            threshold=threshold,
        )


# ── text parsers (AURA/Atom → structured) ──────────────────────────────


def _parse_found_nodes(text: str) -> list:
    """Parse autoui_find output into FoundNode list.

    Output format: "Found N node(s):" then per-match blocks containing
    `vnode_N` ids and kind/label info.
    """
    nodes = []
    # Match vnode ids and their kind/label context
    for m in re.finditer(r"(vnode_\d+)", text):
        vnode_id = m.group(1)
        # Find surrounding context for kind/label
        start = max(0, m.start() - 200)
        context = text[start : m.end() + 200]
        kind = _extract_kind(context)
        label = _extract_label(context)
        nodes.append(FoundNode(vnode_id=vnode_id, kind=kind, label=label, raw=context))
    return nodes


def _extract_kind(context: str) -> str:
    m = re.search(r"\b(button|input|textarea|text|col|row|checkbox|select|table|progress|image)\b", context)
    return m.group(1) if m else "unknown"


def _extract_label(context: str) -> str:
    # Labels appear as label:"..." or content:"..." or quoted text
    m = re.search(r'(?:label|content|value|placeholder):\s*"([^"]*)"', context)
    return m.group(1) if m else ""


def _parse_action_result(text: str) -> ActionResult:
    """Parse autoui_action/type AURA text into ActionResult."""
    status = "ok" if "status: ok" in text else "error"
    element = _extract_field(text, "element") or ""
    action = _extract_field(text, "action") or ""
    handler = _extract_field(text, "handler")
    changes = []
    for m in re.finditer(r"(\S+):\s*(.+?)\s*->\s*(.+)", text):
        changes.append((m.group(1), m.group(2).strip(), m.group(3).strip()))
    return ActionResult(
        status=status, element=element, action=action, handler=handler, state_changes=changes
    )


def _extract_field(text: str, field: str) -> Optional[str]:
    m = re.search(rf"{field}:\s*(\S+)", text)
    return m.group(1) if m else None


def wait_for_server(url: str = MCP_URL, timeout: int = 30) -> bool:
    """Poll the MCP server until it responds or timeout. Returns True if up."""
    for _ in range(timeout):
        try:
            requests.post(
                url,
                json={"jsonrpc": "2.0", "method": "tools/list", "params": {}, "id": 1},
                timeout=2,
            )
            return True
        except (requests.ConnectionError, requests.Timeout):
            time.sleep(1)
    return False
