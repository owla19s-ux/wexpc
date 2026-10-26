"""Self-tests for the standalone WEXPC API mock (not WEXPC product tests)."""

import http.client
import json
import socket
import threading
import unittest

from api_chat_mock import MockServer, TOOL_ID, sse_frames


class MockHarnessTests(unittest.TestCase):
    def setUp(self):
        self.server = None
        self.thread = None

    def tearDown(self):
        if self.server:
            self.server.shutdown()
            self.server.server_close()
            self.thread.join(timeout=2)

    def start_server(self, scenario, hold=0):
        self.server = MockServer(("127.0.0.1", 0), scenario, hold)
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()
        return self.server.server_port

    def post(self, port, messages):
        conn = http.client.HTTPConnection("127.0.0.1", port, timeout=2)
        conn.request("POST", "/v1/chat/completions", json.dumps({
            "model": "mock", "messages": messages, "stream": False,
        }), {"Content-Type": "application/json", "Authorization": "Bearer test-secret"})
        response = conn.getresponse()
        status, body = response.status, json.loads(response.read())
        conn.close()
        return status, body

    def test_text_and_missing_finish_are_distinct(self):
        port = self.start_server("missing-finish")
        status, body = self.post(port, [{"role": "user", "content": "secret prompt"}])
        self.assertEqual(status, 200)
        self.assertNotIn("finish_reason", body["choices"][0])
        self.assertEqual(self.server.last_request_shape["roles"], ["user"])
        self.assertNotIn("secret", str(self.server.last_request_shape))

    def test_tool_roundtrip_requires_matching_assistant_and_tool_ids(self):
        port = self.start_server("tool-roundtrip")
        status, first = self.post(port, [{"role": "user", "content": "read"}])
        self.assertEqual(status, 200)
        self.assertEqual(first["choices"][0]["finish_reason"], "tool_calls")
        call = first["choices"][0]["message"]["tool_calls"][0]
        self.assertEqual(call["id"], TOOL_ID)
        assistant = first["choices"][0]["message"]
        messages = [
            {"role": "user", "content": "read"}, assistant,
            {"role": "tool", "tool_call_id": TOOL_ID, "content": "file content"},
        ]
        status, second = self.post(port, messages)
        self.assertEqual(status, 200)
        self.assertEqual(second["choices"][0]["finish_reason"], "stop")
        messages[-1]["tool_call_id"] = "wrong-id"
        status, error = self.post(port, messages)
        self.assertEqual(status, 400)
        self.assertEqual(error["error"]["code"], "invalid_tool_result")
        self.assertEqual(self.server.requests_seen, 3)

    def test_sse_done_precedes_http_eof(self):
        port = self.start_server("sse-held-open", hold=0.6)
        with socket.create_connection(("127.0.0.1", port), timeout=2) as conn:
            body = json.dumps({"messages": [{"role": "user", "content": "hi"}], "stream": True})
            request = (f"POST /v1/chat/completions HTTP/1.1\r\nHost: 127.0.0.1\r\n"
                       f"Content-Type: application/json\r\nContent-Length: {len(body)}\r\n"
                       f"Connection: close\r\n\r\n{body}")
            conn.sendall(request.encode())
            conn.settimeout(1.5)
            response = b""
            while b"data: [DONE]\n\n" not in response:
                chunk = conn.recv(8192)
                self.assertTrue(chunk, "EOF before [DONE]")
                response += chunk
            self.assertIn(b'"finish_reason":"stop"', response)
            self.assertIn(b'"usage"', response)
            conn.settimeout(0.1)
            with self.assertRaises(socket.timeout):
                conn.recv(1)
            conn.settimeout(1.5)
            self.assertEqual(conn.recv(1), b"", "connection should eventually close")

    def test_sse_tool_intent_and_after_done_fixtures(self):
        tool_stream = "".join(sse_frames("sse-tool-intent"))
        self.assertIn('"finish_reason":"tool_calls"', tool_stream)
        self.assertIn('"id":"call_demo_1"', tool_stream)
        self.assertIn("data: [DONE]", tool_stream)
        invalid_stream = "".join(sse_frames("sse-after-done"))
        self.assertGreater(invalid_stream.index('"content":"illegal"'),
                           invalid_stream.index("data: [DONE]"))

    def test_sse_missing_done_fixture(self):
        port = self.start_server("sse-missing-done")
        with socket.create_connection(("127.0.0.1", port), timeout=2) as conn:
            body = '{"messages":[{"role":"user","content":"hi"}],"stream":true}'
            conn.sendall((f"POST /v1/chat/completions HTTP/1.1\r\nHost: 127.0.0.1\r\n"
                          f"Content-Length: {len(body)}\r\n\r\n{body}").encode())
            conn.settimeout(2)
            chunks = []
            while True:
                chunk = conn.recv(8192)
                if not chunk:
                    break
                chunks.append(chunk)
            raw = b"".join(chunks)
            self.assertIn(b'"finish_reason":"stop"', raw)
            self.assertNotIn(b"data: [DONE]", raw)


if __name__ == "__main__":
    unittest.main()
