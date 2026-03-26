"""Anthropic Claude integration for QBO class prediction.

Ported from Rust src/llm_engine.rs. Uses the Anthropic Python SDK with
streaming to predict the correct QBO Class for each transaction line item.
"""

import json
import logging
from typing import Any

import anthropic

from .config import ANTHROPIC_MODEL
from .models import LlmPrediction, TransactionLine
from . import secrets

logger = logging.getLogger(__name__)


# ---------------------------------------------------------------------------
# JSON extraction (ported from Rust depth-aware brace matcher)
# ---------------------------------------------------------------------------

def extract_first_json_object(raw: str) -> str | None:
    """Find the first top-level ``{...}`` JSON object in *raw*.

    Uses depth-aware brace counting that respects string literals and
    escape sequences. Ignores any trailing hallucinated output from the LLM.

    Returns the substring containing the first balanced JSON object,
    or ``None`` if no balanced object is found.
    """
    start: int | None = None
    for idx, ch in enumerate(raw):
        if ch == "{":
            start = idx
            break

    if start is None:
        return None

    depth = 0
    in_string = False
    escape_next = False

    for i, ch in enumerate(raw[start:], start=start):
        if escape_next:
            escape_next = False
            continue

        if ch == "\\" and in_string:
            escape_next = True
            continue

        if ch == '"':
            in_string = not in_string
            continue

        if in_string:
            continue

        if ch == "{":
            depth += 1
        elif ch == "}":
            depth -= 1
            if depth == 0:
                return raw[start : i + 1]

    # Unbalanced braces
    return None


# ---------------------------------------------------------------------------
# Prompt builders
# ---------------------------------------------------------------------------

def _build_system_prompt(available_classes: list[tuple[str, str]]) -> str:
    """Construct the system prompt with the full list of available QBO classes."""
    class_lines = "\n".join(
        f'  - "{name}" (ID: {cls_id})'
        for cls_id, name in available_classes
    )

    return (
        "You are an expert QBO accountant agent.\n"
        "Your task is to classify a transaction line item into exactly ONE "
        "of the available QBO Classes listed below.\n"
        "You MUST choose from this list — do NOT invent new classes or use "
        "account names.\n\n"
        f"Available QBO Classes:\n{class_lines}\n\n"
        "You MUST output strictly in valid JSON matching this schema:\n"
        "{\n"
        '    "class_ref": "<The exact Name of the class from the list above>",\n'
        '    "reasoning": "<A brief 1 sentence explanation>",\n'
        '    "confidence_score": <A float between 0.0 and 1.0>\n'
        "}\n"
        "Do not output any markdown formatting or extra text. Just the raw JSON object."
    )


def _build_user_prompt(tx: TransactionLine) -> str:
    """Construct the user prompt with clean formatting (N/A for None)."""

    def fmt_opt(val: str | None) -> str:
        return val if val is not None else "N/A"

    def fmt_amt(val: float | None) -> str:
        return f"{val:.2f}" if val is not None else "N/A"

    return (
        f"Transaction Type: {tx.tx_type}\n"
        f"Entity Name: {fmt_opt(tx.entity_name)}\n"
        f"Header Memo: {fmt_opt(tx.header_memo)}\n"
        f"Line Description: {fmt_opt(tx.line_description)}\n"
        f"Account: {fmt_opt(tx.account)}\n"
        f"Amount: {fmt_amt(tx.amount)}"
    )


# ---------------------------------------------------------------------------
# Prediction
# ---------------------------------------------------------------------------

async def predict_class(
    tx: TransactionLine,
    available_classes: list[tuple[str, str]],
) -> LlmPrediction:
    """Predict the QBO Class for a single transaction line item.

    Uses Anthropic's streaming API (``client.messages.stream()``) to generate
    a JSON prediction, then parses it into an :class:`LlmPrediction`.

    Args:
        tx: The transaction line to classify.
        available_classes: List of ``(id, name)`` tuples for active QBO classes.

    Returns:
        An :class:`LlmPrediction` with ``class_ref``, ``reasoning``, and
        ``confidence_score``.

    Raises:
        ValueError: If the LLM output cannot be parsed into valid JSON.
        anthropic.APIError: On Anthropic API failures.
    """
    api_key = secrets.get_anthropic_api_key()
    client = anthropic.AsyncAnthropic(api_key=api_key)

    system_prompt = _build_system_prompt(available_classes)
    user_prompt = _build_user_prompt(tx)

    # Stream the response and collect the full text
    collected_text = ""
    async with client.messages.stream(
        model=ANTHROPIC_MODEL,
        max_tokens=512,
        temperature=0.0,
        system=system_prompt,
        messages=[{"role": "user", "content": user_prompt}],
    ) as stream:
        async for text in stream.text_stream:
            collected_text += text

    # Extract the first JSON object (LLMs may wrap in markdown or hallucinate extra text)
    json_payload = extract_first_json_object(collected_text)
    if json_payload is None:
        logger.error(
            "No JSON object found in LLM output. Raw LLM Output:\n%s",
            collected_text,
        )
        raise ValueError(
            f"LLM did not return a valid JSON object. Raw output: {collected_text!r}"
        )

    try:
        parsed: dict[str, Any] = json.loads(json_payload)
    except json.JSONDecodeError as exc:
        logger.error(
            "Failed to decode JSON from LLM output. "
            "Extracted payload:\n%s\nRaw LLM Output:\n%s",
            json_payload,
            collected_text,
        )
        raise ValueError(
            f"LLM returned invalid JSON: {exc}. Raw output: {collected_text!r}"
        ) from exc

    try:
        prediction = LlmPrediction(
            class_ref=parsed["class_ref"],
            reasoning=parsed["reasoning"],
            confidence_score=float(parsed["confidence_score"]),
        )
    except (KeyError, TypeError, ValueError) as exc:
        logger.error(
            "Failed to parse LLM prediction fields. "
            "Parsed JSON: %s\nRaw LLM Output:\n%s",
            parsed,
            collected_text,
        )
        raise ValueError(
            f"LLM JSON missing required fields: {exc}. "
            f"Parsed: {parsed!r}"
        ) from exc

    return prediction
