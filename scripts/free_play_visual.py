#!/usr/bin/env python3
"""Run the semantic Free Play smoke test through egui-mcp.

The test starts the real native app under Xvfb with Mesa software rendering,
drives it through the accessibility tree, and writes review screenshots under
VISUAL_ARTIFACT_DIR (or target/visual by default).
"""

from __future__ import annotations

import json
import os
import shutil
import signal
import socket
import subprocess
import sys
import tempfile
import time
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parent.parent
APP_LABEL = "21 Pro · Blackjack Strategy Trainer"
ACTION_LABELS = (
    "Hit",
    "Stand",
    "Double",
    "Split",
    "Surrender",
    "Take insurance",
    "No insurance",
)


class VisualTestError(RuntimeError):
    """A failed native visual assertion or unavailable visual prerequisite."""


class McpClient:
    def __init__(self) -> None:
        server = shutil.which("egui-mcp")
        if server is None:
            raise VisualTestError(
                "egui-mcp is required; install egui_mcp 0.2.0 with "
                "`cargo install egui_mcp --version 0.2.0 --locked`."
            )
        self.process = subprocess.Popen(
            [server],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            text=True,
            bufsize=1,
        )
        self.next_id = 1
        self._send(
            {
                "jsonrpc": "2.0",
                "id": self.next_id,
                "method": "initialize",
                "params": {
                    "protocolVersion": "2024-11-05",
                    "capabilities": {},
                    "clientInfo": {"name": "21pro-free-play-visual", "version": "1"},
                },
            }
        )
        self._read_response(self.next_id)
        self.next_id += 1
        self._send({"jsonrpc": "2.0", "method": "notifications/initialized"})

    def _send(self, message: dict[str, Any]) -> None:
        if self.process.stdin is None:
            raise VisualTestError("egui-mcp stdin is unavailable")
        self.process.stdin.write(json.dumps(message) + "\n")
        self.process.stdin.flush()

    def _read_response(self, request_id: int, timeout: float = 45.0) -> dict[str, Any]:
        if self.process.stdout is None:
            raise VisualTestError("egui-mcp stdout is unavailable")
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            line = self.process.stdout.readline()
            if not line:
                raise VisualTestError("egui-mcp exited before returning a response")
            response = json.loads(line)
            if response.get("id") != request_id:
                continue
            if "error" in response:
                raise VisualTestError(f"egui-mcp error: {response['error']}")
            result = response.get("result", {})
            if result.get("isError"):
                detail = " ".join(
                    item.get("text", "")
                    for item in result.get("content", [])
                    if item.get("type") == "text"
                )
                raise VisualTestError(f"egui-mcp tool error: {detail or result}")
            structured = result.get("structuredContent")
            if structured is not None:
                return structured
            for item in result.get("content", []):
                if item.get("type") == "text":
                    try:
                        return json.loads(item["text"])
                    except json.JSONDecodeError:
                        return {"text": item["text"]}
            return {}
        raise VisualTestError(f"timed out waiting for egui-mcp response {request_id}")

    def call(self, name: str, arguments: dict[str, Any] | None = None) -> dict[str, Any]:
        request_id = self.next_id
        self.next_id += 1
        self._send(
            {
                "jsonrpc": "2.0",
                "id": request_id,
                "method": "tools/call",
                "params": {"name": name, "arguments": arguments or {}},
            }
        )
        return self._read_response(request_id)

    def attach(self, port: int) -> None:
        result = self.call(
            "attach",
            {"host": "127.0.0.1", "port": port, "timeout_secs": 45},
        )
        attached = result.get("attached", {})
        if attached.get("label") != APP_LABEL:
            raise VisualTestError(f"unexpected app label: {attached!r}")

    def disconnect(self) -> None:
        try:
            self.call("disconnect")
        except VisualTestError:
            pass

    def close(self) -> None:
        self.disconnect()
        if self.process.poll() is None:
            self.process.terminate()
            try:
                self.process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait()

    def query(self, **filters: Any) -> list[dict[str, Any]]:
        filters.setdefault("visible_only", True)
        return self.call("query_tree", filters).get("nodes", [])

    def click(self, **target: Any) -> None:
        self.call("click", target)

    def wait_for(self, **filters: Any) -> None:
        filters.setdefault("timeout_secs", 10)
        filters.setdefault("min_steps", 2)
        self.call("wait_for", filters)

    def type_text(self, text: str, **target: Any) -> None:
        self.call("type_text", {**target, "text": text})

    def press_key(self, key: str, **modifiers: Any) -> None:
        arguments: dict[str, Any] = {"key": key}
        if modifiers:
            arguments["modifiers"] = modifiers
        self.call("press_key", arguments)

    def scroll(self, content: str, amount: int = 900) -> None:
        self.call(
            "scroll",
            {"role": "Label", "content_contains": content, "delta": {"x": 0, "y": amount}},
        )

    def resize(self, width: int, height: int) -> None:
        self.call("resize", {"width": width, "height": height})

    def screenshot(self, path: Path) -> None:
        path.parent.mkdir(parents=True, exist_ok=True)
        self.call("screenshot", {"save_path": str(path)})
        if not path.is_file() or path.stat().st_size == 0:
            raise VisualTestError(f"screenshot was not written: {path}")


def content(node: dict[str, Any]) -> str:
    return str(node.get("label") or node.get("value") or "")


def require_nodes(client: McpClient, text: str, *, role: str | None = None) -> list[dict[str, Any]]:
    filters: dict[str, Any] = {"content_contains": text}
    if role is not None:
        filters["role"] = role
    nodes = client.query(**filters)
    if not nodes:
        raise VisualTestError(f"missing {role or 'node'} containing {text!r}")
    return nodes


def require_text(client: McpClient, text: str) -> None:
    require_nodes(client, text)


def button(client: McpClient, text: str) -> dict[str, Any]:
    nodes = require_nodes(client, text, role="Button")
    if len(nodes) != 1:
        raise VisualTestError(f"expected one button containing {text!r}, got {nodes!r}")
    return nodes[0]


def assert_button_state(client: McpClient, text: str, disabled: bool) -> None:
    node = button(client, text)
    if node.get("disabled") != disabled:
        state = "disabled" if disabled else "enabled"
        raise VisualTestError(f"button {text!r} should be {state}: {node!r}")


def session_snapshot(client: McpClient) -> list[tuple[Any, ...]]:
    nodes = client.query(visible_only=False, limit=500)
    markers = (
        "$",
        "Cards:",
        "Cards remaining",
        "Pending wager",
        "Choose an action",
        "Numeric session history",
        "Cumulative P/L",
    ) + ACTION_LABELS
    return sorted(
        (
            node.get("role"),
            node.get("label"),
            node.get("value"),
            node.get("disabled"),
            node.get("hidden"),
        )
        for node in nodes
        if node.get("role") == "Image"
        or any(marker.lower() in content(node).lower() for marker in markers)
    )


def assert_initial_play(client: McpClient) -> None:
    require_text(client, "Free Play")
    require_text(client, "AVAILABLE FUNDS")
    require_text(client, "$1000.00")
    for chip in ("Red $5.00", "Green $25.00", "Black $100.00", "Yellow $1000.00"):
        require_nodes(client, chip, role="Button")
    require_text(client, "Session P/L")
    require_text(client, "Numeric session history")
    require_text(client, "Cumulative P/L")
    if not any(content(node) == "0" for node in client.query(role="Label")):
        raise VisualTestError("initial numeric session history has no Round 0 row")
    require_text(client, "$0.00")
    assert_button_state(client, "Deal", disabled=True)


def assert_active_round(client: McpClient, available: str, committed: str, cards: str) -> None:
    require_text(client, available)
    require_text(client, committed)
    require_text(client, cards)
    require_text(client, "Choose an action")
    action_nodes = [
        node
        for node in client.query(role="Button")
        if any(action.lower() in content(node).lower() for action in ACTION_LABELS)
    ]
    if not action_nodes:
        raise VisualTestError("no Free Play action button is discoverable")


def assert_hidden_hole_card(client: McpClient) -> None:
    image_nodes = client.query(role="Image")
    labels = [content(node) for node in image_nodes]
    if not any("face-down card" in label for label in labels):
        raise VisualTestError(f"dealer hole card is not represented as face-down: {labels!r}")


def open_reset(client: McpClient) -> None:
    client.scroll("Session P/L")
    client.click(content_contains="Reset session / new bankroll")
    client.wait_for(role="TextInput", timeout_secs=5)


def replace_reset_input(client: McpClient, value: str) -> None:
    client.click(role="TextInput")
    client.press_key("Home")
    client.press_key("End", shift=True)
    client.press_key("Backspace")
    client.type_text(value)


def confirm_invalid_reset(client: McpClient) -> None:
    open_reset(client)
    replace_reset_input(client, "1.001")
    client.click(content_contains="Confirm reset")
    client.wait_for(content_contains="whole cents", timeout_secs=5)
    require_text(client, "$870.00")
    require_text(client, "$130.00")
    require_text(client, "308")


def cancel_reset_without_change(client: McpClient) -> None:
    client.click(content_contains="Cancel")
    client.wait_for(content_contains="Choose an action", timeout_secs=5)
    require_text(client, "$870.00")
    require_text(client, "$130.00")
    require_text(client, "308")


def navigate_play(client: McpClient) -> None:
    client.click(content_contains="05   Play")
    client.wait_for(content_contains="Free Play", timeout_secs=10)


def start_app(profile: Path, runtime: Path, port: int) -> subprocess.Popen[str]:
    if shutil.which("xvfb-run") is None:
        raise VisualTestError("xvfb-run is required for the native visual smoke")
    if shutil.which("cargo") is None:
        raise VisualTestError("cargo is required for the native visual smoke")
    log_path = profile.parent / f"app-{port}.log"
    log = log_path.open("w", encoding="utf-8")
    environment = os.environ.copy()
    for key in ("DISPLAY", "WAYLAND_DISPLAY", "WAYLAND_SOCKET", "EFRAME_SCREENSHOT_TO"):
        environment.pop(key, None)
    environment.update(
        {
            "LIBGL_ALWAYS_SOFTWARE": "1",
            "WINIT_X11_SCALE_FACTOR": "1",
            "XDG_RUNTIME_DIR": str(runtime),
            "TWENTY_ONE_PRO_DATA_DIR": str(profile),
            "EGUI_INSPECTION": f"127.0.0.1:{port}",
        }
    )
    process = subprocess.Popen(
        [
            "xvfb-run",
            "-a",
            "-s",
            "-screen 0 1280x1024x24 -dpi 96 -nolisten tcp",
            "cargo",
            "run",
            "--locked",
            "--features",
            "dev-inspection",
        ],
        cwd=ROOT,
        env=environment,
        stdout=log,
        stderr=subprocess.STDOUT,
        start_new_session=True,
        text=True,
    )
    process._visual_log = log  # type: ignore[attr-defined]
    return process


def stop_app(process: subprocess.Popen[str]) -> None:
    if process.poll() is None:
        os.killpg(process.pid, signal.SIGTERM)
        try:
            process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            os.killpg(process.pid, signal.SIGKILL)
            process.wait()
    log = getattr(process, "_visual_log", None)
    if log is not None:
        log.close()


def free_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
        sock.bind(("127.0.0.1", 0))
        return int(sock.getsockname()[1])


def deal_mixed_wager_until_active(client: McpClient, bankroll: str = "1000.00") -> None:
    for attempt in range(12):
        if attempt:
            reset_to_bankroll(client, bankroll)
        client.click(content_contains="Red $5.00")
        client.click(content_contains="Green $25.00")
        client.click(content_contains="Black $100.00")
        require_text(client, "Pending wager: $130.00")
        assert_button_state(client, "Yellow $1000.00", disabled=True)
        client.click(content_contains="Deal  [Enter]")
        client.wait_for(min_steps=3, timeout_secs=10)
        if client.query(content_contains="Choose an action"):
            return
        if not client.query(role="Button", content_contains="Deal next hand"):
            raise VisualTestError("dealing did not produce an action state or a settled round")
    raise VisualTestError("random shoe did not produce an unfinished round after 12 deals")


def run() -> None:
    artifact_dir = Path(os.environ.get("VISUAL_ARTIFACT_DIR", ROOT / "target/visual"))
    artifact_dir.mkdir(parents=True, exist_ok=True)
    port = int(os.environ.get("EGUI_INSPECTION_PORT", free_port()))
    client = McpClient()
    process: subprocess.Popen[str] | None = None
    with tempfile.TemporaryDirectory(prefix="21pro-free-play-visual-") as temporary:
        temporary_path = Path(temporary)
        profile = temporary_path / "profile"
        runtime = temporary_path / "runtime"
        profile.mkdir()
        runtime.mkdir()
        try:
            print("Starting the native app under Xvfb/Mesa...")
            process = start_app(profile, runtime, port)
            client.attach(port)
            client.wait_for(min_steps=3, timeout_secs=5)
            print("Happy path: Play, chips, deal, hidden card, and narrow layout")
            navigate_play(client)
            assert_initial_play(client)
            deal_mixed_wager_until_active(client)
            assert_active_round(client, "$870.00", "$130.00", "308")
            assert_hidden_hole_card(client)
            client.screenshot(artifact_dir / "free-play-dealt.png")
            confirm_invalid_reset(client)
            cancel_reset_without_change(client)
            client.resize(920, 700)
            client.wait_for(content_contains="Free Play", timeout_secs=5)
            client.scroll("Choose an action")
            require_text(client, "Free Play")
            require_text(client, "Choose an action")
            client.screenshot(artifact_dir / "free-play-narrow.png")
            client.resize(1180, 860)
            client.wait_for(content_contains="Free Play", timeout_secs=5)
            before_restart = session_snapshot(client)
            client.disconnect()
            stop_app(process)
            process = None
            client.close()
            client = McpClient()
            port = free_port()

            print("Restart path: restore the unfinished round from the same profile")
            process = start_app(profile, runtime, port)
            client.attach(port)
            navigate_play(client)
            after_restart = session_snapshot(client)
            if after_restart != before_restart:
                before_set = set(before_restart)
                after_set = set(after_restart)
                print(f"restart-only nodes: {sorted(after_set - before_set)!r}", file=sys.stderr)
                print(f"missing-after-restart: {sorted(before_set - after_set)!r}", file=sys.stderr)
                raise VisualTestError(
                    "restarting the disposable profile changed the unfinished round, funds, "
                    "cards, or accessibility state"
                )
            assert_active_round(client, "$870.00", "$130.00", "308")

            print("Failure path: invalid/cancelled reset and unaffordable keyboard action")
            reset_to_bankroll(client, "5.00")
            client.click(content_contains="Red $5.00")
            client.click(content_contains="Deal  [Enter]")
            client.wait_for(min_steps=3, timeout_secs=10)
            for _ in range(12):
                insurance = client.query(role="Button", content_contains="No insurance")
                if insurance:
                    no_insurance = insurance[0]
                    if not no_insurance.get("disabled"):
                        client.click(content_contains="No insurance")
                        client.wait_for(min_steps=3, timeout_secs=5)
                double_nodes = client.query(role="Button", content_contains="Double")
                if double_nodes:
                    double = double_nodes[0]
                    if double.get("disabled"):
                        break
                reset_to_bankroll(client, "5.00")
                client.click(content_contains="Red $5.00")
                client.click(content_contains="Deal  [Enter]")
                client.wait_for(min_steps=3, timeout_secs=10)
            else:
                raise VisualTestError("could not reach a legal Double action with no available funds")
            double = button(client, "Double")
            if not double.get("disabled"):
                raise VisualTestError("Double was affordable after a $5 wager")
            before_keyboard = session_snapshot(client)
            client.press_key("D")
            client.wait_for(min_steps=3, timeout_secs=5)
            if session_snapshot(client) != before_keyboard:
                raise VisualTestError("keyboard Double changed state while the action was unaffordable")
            client.screenshot(artifact_dir / "free-play-affordability.png")
            print("Free Play visual smoke passed")
            print(f"Screenshots: {artifact_dir / 'free-play-dealt.png'},")
            print(f"            {artifact_dir / 'free-play-narrow.png'},")
            print(f"            {artifact_dir / 'free-play-affordability.png'}")
        finally:
            client.close()
            if process is not None:
                stop_app(process)


def reset_to_bankroll(client: McpClient, value: str) -> None:
    open_reset(client)
    replace_reset_input(client, value)
    client.click(content_contains="Confirm reset")
    client.wait_for(content_contains="Free Play", timeout_secs=10)
    if not client.query(content_contains=f"${value}"):
        visible = [content(node) for node in client.query(visible_only=False, limit=500)]
        raise VisualTestError(f"reset to ${value} did not apply; visible content: {visible!r}")
    require_text(client, "Cards remaining")
    require_text(client, "312")
    require_text(client, "Pending wager: $0.00")
    require_text(client, "Round")
    require_text(client, "$0.00")


def main() -> int:
    try:
        run()
    except (VisualTestError, AssertionError, OSError, subprocess.SubprocessError) as error:
        print(f"Free Play visual smoke failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
