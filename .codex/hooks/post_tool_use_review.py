#!/usr/bin/env python3

import json
import sys


def main() -> int:
    payload = json.load(sys.stdin)
    command = (
        payload.get("tool_input", {}).get("command")
        or payload.get("tool_input.command")
        or ""
    )
    response = payload.get("tool_response")
    response_text = response if isinstance(response, str) else json.dumps(response)

    message = None
    if command.startswith("cargo fmt"):
        message = (
            "Formatting completed. Review the diff, ensure only intended files changed, "
            "and update the active ExecPlan before finishing."
        )
    elif command.startswith("cargo clippy") and "warning:" in response_text:
        message = (
            "Clippy warnings are allowed by the local git gate in v1, but they should be "
            "called out in the final validation summary if they matter."
        )
    elif command.startswith("cargo test") and "test result: FAILED" in response_text:
        message = "Tests failed. Fix the issue or report the failure before continuing."

    if message is not None:
        json.dump(
            {
                "hookSpecificOutput": {
                    "hookEventName": "PostToolUse",
                    "additionalContext": message,
                }
            },
            sys.stdout,
        )

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
