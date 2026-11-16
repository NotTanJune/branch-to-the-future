from __future__ import annotations

import os
import subprocess
from collections.abc import Sequence

from .redact import scrub_secrets
from .schemas import JsonEnvelope, parse_envelope_json


class BrfTransportError(RuntimeError):
    pass


class BrfTransport:
    def __init__(
        self,
        command: str | Sequence[str] | None = None,
        *,
        scan_timeout: float | None = 120,
        analyze_timeout: float | None = 600,
    ) -> None:
        if command is None:
            command = os.environ.get("BRF_BINARY", "brf")
        self.command = [command] if isinstance(command, str) else list(command)
        if not self.command or not str(self.command[0]).strip():
            raise BrfTransportError("brf binary is empty; set BRF_BINARY or --brf-binary")
        self.scan_timeout = scan_timeout
        self.analyze_timeout = analyze_timeout

    def scan(
        self,
        repo: str,
        *,
        max_file_bytes: int | None = None,
        ignore: Sequence[str] = (),
    ) -> JsonEnvelope:
        args = ["scan", "--json"]
        if max_file_bytes is not None:
            args.extend(["--max-file-bytes", str(max_file_bytes)])
        for pattern in ignore:
            args.extend(["--ignore", pattern])
        args.extend(["--", repo])
        return self._run(args, self.scan_timeout)

    def analyze(
        self,
        repo: str,
        change: str,
        *,
        max_file_bytes: int | None = None,
        ignore: Sequence[str] = (),
        max_prompt_files: int | None = None,
        max_output_tokens: int | None = None,
        text_model: str | None = None,
    ) -> JsonEnvelope:
        args = ["analyze", "--json", f"--change={change}"]
        if max_file_bytes is not None:
            args.extend(["--max-file-bytes", str(max_file_bytes)])
        if max_prompt_files is not None:
            args.extend(["--max-prompt-files", str(max_prompt_files)])
        if max_output_tokens is not None:
            args.extend(["--max-output-tokens", str(max_output_tokens)])
        if text_model is not None:
            args.extend(["--text-model", text_model])
        for pattern in ignore:
            args.extend(["--ignore", pattern])
        args.extend(["--", repo])
        return self._run(args, self.analyze_timeout)

    def _run(self, args: Sequence[str], timeout: float | None = None) -> JsonEnvelope:
        command = [*self.command, *args]
        try:
            result = subprocess.run(
                command,
                check=False,
                capture_output=True,
                timeout=timeout,
            )
        except subprocess.TimeoutExpired as exc:
            raise BrfTransportError(f"brf timed out after {timeout}s") from exc
        except OSError as exc:
            raise BrfTransportError(f"failed to execute brf: {exc}") from exc

        stdout = result.stdout.decode("utf-8", errors="replace")
        if result.returncode != 0:
            stderr = _redact(result.stderr.decode("utf-8", errors="replace").strip())
            raise BrfTransportError(
                f"brf exited with status {result.returncode}: {stderr}"
            )
        try:
            return parse_envelope_json(stdout)
        except ValueError as exc:
            raise BrfTransportError(str(exc)) from exc


def _redact(text: str) -> str:
    return scrub_secrets(text)
