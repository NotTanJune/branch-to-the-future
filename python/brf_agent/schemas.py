from __future__ import annotations

import json
from enum import Enum
from typing import Any

from pydantic import BaseModel, ConfigDict, Field, ValidationError, field_validator

SCHEMA_VERSION = 1


class StrictModel(BaseModel):
    model_config = ConfigDict(extra="forbid")


class FileKind(str, Enum):
    java_script = "java_script"
    type_script = "type_script"
    python = "python"
    rust = "rust"
    config = "config"
    route = "route"
    test = "test"
    schema = "schema"
    worker = "worker"
    service = "service"
    controller = "controller"
    ui_component = "ui_component"
    unknown = "unknown"


class RiskLevel(str, Enum):
    low = "low"
    medium = "medium"
    high = "high"


class Complexity(str, Enum):
    low = "low"
    medium = "medium"
    high = "high"


class RouteInfo(StrictModel):
    path: str
    method: str
    route: str


class RiskSignal(StrictModel):
    path: str
    signal: str


class RepoFile(StrictModel):
    path: str
    kind: FileKind
    size: int = Field(ge=0)
    symbols: list[str]
    imports: list[str]
    snippets: list[str]


class RepoModel(StrictModel):
    repo_name: str
    root_path: str
    frameworks: list[str]
    files: list[RepoFile]
    routes: list[RouteInfo]
    tests: list[str]
    config_files: list[str]
    risk_signals: list[RiskSignal]


class ArchitectureStage(StrictModel):
    label: str
    responsibilities: list[str] = Field(default_factory=list)
    files: list[str] = Field(default_factory=list)


class ImpactFile(StrictModel):
    path: str
    reason: str
    impact_score: int
    confidence: int
    risk: RiskLevel
    change_needed: str

    @field_validator("impact_score", "confidence")
    @classmethod
    def score_between_zero_and_hundred(cls, value: int) -> int:
        if isinstance(value, bool) or not 0 <= value <= 100:
            raise ValueError("score fields must be integers from 0..100")
        return value


class ImplementationFuture(StrictModel):
    name: str
    description: str
    complexity: Complexity
    risk: RiskLevel
    architecture: list[ArchitectureStage] = Field(default_factory=list)
    affected_files: list[str]
    benefits: list[str] = Field(default_factory=list)
    drawbacks: list[str] = Field(default_factory=list)
    patch_plan: list[str] = Field(default_factory=list)
    test_plan: list[str] = Field(default_factory=list)


class ImpactAnalysis(StrictModel):
    summary: str
    current_architecture: list[ArchitectureStage] = Field(default_factory=list)
    impact_path: list[ImpactFile]
    risk_summary: list[str]
    tests_to_add: list[str]
    futures: list[ImplementationFuture]
    recommended_future: str


class JsonLimits(StrictModel):
    max_file_bytes: int = Field(ge=0)
    ignore: list[str]
    max_prompt_files: int | None = Field(default=None, ge=0)


class JsonModel(StrictModel):
    provider: str | None = None
    name: str | None = None
    reasoning_effort: str | None = None
    max_output_tokens: int | None = Field(default=None, ge=0)


class JsonEnvelope(StrictModel):
    schema_version: int
    tool: str
    created_at: str
    change_request: str
    repo_model: RepoModel
    impact_analysis: ImpactAnalysis | None
    limits: JsonLimits
    model: JsonModel
    warnings: list[str]


def parse_envelope_json(raw: str) -> JsonEnvelope:
    try:
        data: Any = json.loads(raw)
    except json.JSONDecodeError as exc:
        raise ValueError(f"invalid JSON from brf: {exc.msg}") from exc

    try:
        envelope = JsonEnvelope.model_validate(data)
    except ValidationError as exc:
        raise ValueError(f"invalid brf envelope: {exc}") from exc

    if envelope.schema_version != SCHEMA_VERSION:
        raise ValueError(
            f"schema_version {envelope.schema_version} is not supported; expected {SCHEMA_VERSION}"
        )
    return envelope
