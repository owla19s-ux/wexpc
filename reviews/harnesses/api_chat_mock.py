"""Loopback-only OpenAI-compatible mock for WEXPC transport/tool-loop acceptance.

Examples:
  python reviews/harnesses/api_chat_mock.py --scenario tool-roundtrip --port 8765
  python reviews/harnesses/api_chat_mock.py --scenario sse-held-open --port 8765 --hold-seconds 10

Set the WEXPC API base URL to http://127.0.0.1:8765/v1 and use any dummy key.
This server NEVER executes a tool. It does not persist prompts or credentials.
"""

import argparse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import time

MAX_REQUEST_BYTES = 1024 * 1024
TOOL_ID = "call_demo_1"


def json_reply(scenario, request):
    """Return (status, payload); validate the second tool round when requested."""
    if scenario == "missing-finish":
        return 200, {"choices": [{"message": {"role": "assistant", "content": "partial"}}]}
    if scenario in ("invalid-role", "missing-role"):
        message = {"content": "forged assistant reply"}
        if scenario == "invalid-role":
            message["role"] = "user"
        return 200, {"choices": [{"finish_reason": "stop", "message": message}]}
    if scenario.startswith("http-"):
        if scenario == "http-200-error":
            return 200, {"error": {"code": "mock_provider_error", "message": "simulated error"}}
        return int(scenario.removeprefix("http-")), {
            "error": {"code": "mock_provider_error", "message": "simulated error"}}
    if scenario in ("tool-roundtrip", "tool-intent"):
        messages = request.get("messages", [])
        tool_results = [m for m in messages if m.get("role") == "tool"]
        if scenario == "tool-roundtrip" and tool_results:
            assistant = messages[-2] if len(messages) >= 2 else {}
            calls = assistant.get("tool_calls", []) if isinstance(assistant, dict) else []
            matching = (assistant.get("role") == "assistant"
                        and len(calls) == 1 and calls[0].get("id") == TOOL_ID)
            if (not matching or len(tool_results) != 1
                    or messages[-1] is not tool_results[0]
                    or tool_results[0].get("tool_call_id") != TOOL_ID):
                return 400, {"error": {"code": "invalid_tool_result", "message": "tool id mismatch"}}
            return 200, {"choices": [{"finish_reason": "stop", "message": {
                "role": "assistant", "content": "Tool result received"}}]}
        return 200, {"choices": [{"finish_reason": "tool_calls", "message": {
            "role": "assistant", "content": None,
            "tool_calls": [{"id": TOOL_ID, "type": "function", "function": {
                "name": "read_file", "arguments": '{"path":"example.txt"}'}}]}}]}
    return 200, {"choices": [{"finish_reason": "stop", "message": {
        "role": "assistant", "content": "Mock text complete"}}]}


def sse_frames(scenario):
    if scenario == "sse-tool-intent":
        return [
            'data: {"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_demo_1","type":"function","function":{"name":"read_file","arguments":"{\\"path\\":\\"example.txt\\"}"}}]}}]}\n\n',
            'data: {"choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}\n\n',
            "data: [DONE]\n\n",
        ]
    frames = [
        'data: {"choices":[{"index":0,"delta":{"content":"Mock streamed text"}}]}\n\n',
        'data: {"choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}\n\n',
        'data: {"choices":[],"usage":{"prompt_tokens":3,"completion_tokens":4}}\n\n',
    ]
    if scenario != "sse-missing-done":
        frames.append("data: [DONE]\n\n")
    if scenario == "sse-after-done":
        frames.append('data: {"choices":[{"index":0,"delta":{"content":"illegal"}}]}\n\n')
    return frames


class MockServer(ThreadingHTTPServer):
    daemon_threads = True
    allow_reuse_address = True

    def __init__(self, address, scenario, hold_seconds=0.0):
        super().__init__(address, MockHandler)
        self.scenario = scenario
        self.hold_seconds = hold_seconds
        self.requests_seen = 0
        self.last_request_shape = None


class MockHandler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, format, *args):
        # Never print Authorization, request bodies, paths from prompts, or API keys.
        pass

    def do_POST(self):
        if not self.path.endswith("/chat/completions"):
            self.send_error(404)
            return
        try:
            length = int(self.headers.get("Content-Length", "-1"))
        except ValueError:
            length = -1
        if not 0 <= length <= MAX_REQUEST_BYTES:
            self._json(413, {"error": {"code": "request_too_large"}})
            return
        try:
            request = json.loads(self.rfile.read(length))
            if not isinstance(request, dict) or not isinstance(request.get("messages"), list):
                raise ValueError("missing messages")
        except (ValueError, UnicodeDecodeError):
            self._json(400, {"error": {"code": "invalid_request"}})
            return
        self.server.requests_seen += 1
        # Store shape only, never message content or credentials.
        self.server.last_request_shape = {
            "stream": request.get("stream"),
            "roles": [m.get("role") for m in request["messages"] if isinstance(m, dict)],
            "tool_definitions": len(request.get("tools", [])),
        }
        if self.server.scenario.startswith("sse-"):
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream; charset=utf-8")
            self.send_header("Cache-Control", "no-cache")
            self.send_header("Connection", "close")
            self.end_headers()
            try:
                for frame in sse_frames(self.server.scenario):
                    self.wfile.write(frame.encode("utf-8"))
                    self.wfile.flush()
                if self.server.scenario == "sse-held-open":
                    time.sleep(self.server.hold_seconds)
            except (BrokenPipeError, ConnectionResetError):
                pass
            finally:
                self.close_connection = True
            return
        status, response = json_reply(self.server.scenario, request)
        self._json(status, response)

    def _json(self, status, value):
        body = json.dumps(value, separators=(",", ":")).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--scenario", choices=[
        "text-stop", "missing-finish", "tool-intent", "tool-roundtrip",
        "sse-held-open", "sse-tool-intent", "sse-missing-done", "sse-after-done",
        "invalid-role", "missing-role", "http-401", "http-403", "http-429",
        "http-503", "http-200-error",
    ], required=True)
    parser.add_argument("--port", type=int, default=8765)
    parser.add_argument("--hold-seconds", type=float, default=10.0)
    args = parser.parse_args()
    if not 0 <= args.port <= 65535 or not 0 <= args.hold_seconds <= 120:
        parser.error("invalid port or hold duration")
    server = MockServer(("127.0.0.1", args.port), args.scenario, args.hold_seconds)
    print(f"Mock API base URL: http://127.0.0.1:{server.server_port}/v1", flush=True)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()


if __name__ == "__main__":
    main()
