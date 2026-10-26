"""HTTP-level adversarial fixture tests; these do not test WEXPC's Rust client."""

import http.client
import json
import threading
import unittest

from api_chat_mock import MockServer


class ApiErrorRoleMockTests(unittest.TestCase):
    def request_scenario(self, scenario):
        server = MockServer(("127.0.0.1", 0), scenario)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            conn = http.client.HTTPConnection("127.0.0.1", server.server_port, timeout=2)
            try:
                body = json.dumps({"model": "mock", "messages": [
                    {"role": "user", "content": "private prompt"}], "stream": False})
                conn.request("POST", "/v1/chat/completions", body, {
                    "Content-Type": "application/json",
                    "Authorization": "Bearer private-test-key",
                })
                response = conn.getresponse()
                status = response.status
                payload = json.loads(response.read())
            finally:
                conn.close()
            self.assertEqual(server.requests_seen, 1)
            self.assertNotIn("private prompt", str(server.last_request_shape))
            self.assertNotIn("private-test-key", str(server.last_request_shape))
            return status, payload
        finally:
            server.shutdown()
            server.server_close()
            thread.join(timeout=2)

    def test_invalid_and_missing_assistant_role(self):
        for scenario in ("invalid-role", "missing-role"):
            with self.subTest(scenario=scenario):
                status, payload = self.request_scenario(scenario)
                self.assertEqual(status, 200)
                choice = payload["choices"][0]
                self.assertEqual(choice["finish_reason"], "stop")
                self.assertNotEqual(choice["message"].get("role"), "assistant")
                self.assertTrue(choice["message"]["content"])

    def test_http_error_status_fixtures(self):
        for status in (401, 403, 429, 503):
            with self.subTest(status=status):
                actual, payload = self.request_scenario(f"http-{status}")
                self.assertEqual(actual, status)
                self.assertEqual(payload["error"]["code"], "mock_provider_error")
                self.assertNotIn("choices", payload)

    def test_http_200_provider_error_body(self):
        status, payload = self.request_scenario("http-200-error")
        self.assertEqual(status, 200)
        self.assertIn("error", payload)
        self.assertNotIn("choices", payload)


if __name__ == "__main__":
    unittest.main()
