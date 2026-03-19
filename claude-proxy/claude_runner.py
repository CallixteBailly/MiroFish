"""
Module d'appel à la CLI Claude Code via subprocess.
Traduit les messages OpenAI en appel `claude -p` et parse la réponse.
"""

import json
import logging
import os
import re
import subprocess
import uuid
from concurrent.futures import ThreadPoolExecutor

logger = logging.getLogger(__name__)

# Pool de workers pour limiter les appels concurrents
_executor = None


def get_executor():
    global _executor
    if _executor is None:
        max_workers = int(os.environ.get("CLAUDE_PROXY_MAX_WORKERS", "4"))
        _executor = ThreadPoolExecutor(max_workers=max_workers)
    return _executor


# Mapping des noms de modèles OpenAI → Claude
MODEL_MAP = {
    # Alias courts
    "sonnet": "sonnet",
    "opus": "opus",
    "haiku": "haiku",
    # Noms complets Claude
    "claude-sonnet-4-6": "claude-sonnet-4-6",
    "claude-opus-4-6": "claude-opus-4-6",
    "claude-haiku-4-5-20251001": "claude-haiku-4-5-20251001",
    # Mapping OpenAI → Claude (pour compatibilité)
    "gpt-4o": "sonnet",
    "gpt-4o-mini": "sonnet",
    "gpt-4": "opus",
    "gpt-4-turbo": "opus",
    "gpt-3.5-turbo": "haiku",
}

DEFAULT_MODEL = os.environ.get("CLAUDE_PROXY_MODEL", "sonnet")
DEFAULT_TIMEOUT = int(os.environ.get("CLAUDE_PROXY_TIMEOUT", "120"))


def format_messages_to_prompt(messages):
    """
    Convertit une liste de messages OpenAI en (system_prompt, user_prompt).

    Les messages system sont concaténés en system_prompt.
    Les messages user/assistant sont formatés en conversation.
    """
    system_parts = []
    conversation_parts = []

    for msg in messages:
        role = msg.get("role", "user")
        content = msg.get("content", "")

        # Gérer le content qui est une liste (vision API format)
        if isinstance(content, list):
            text_parts = []
            for part in content:
                if isinstance(part, dict) and part.get("type") == "text":
                    text_parts.append(part["text"])
            content = "\n".join(text_parts)

        if role == "system":
            system_parts.append(content)
        elif role == "assistant":
            conversation_parts.append(f"Assistant: {content}")
        elif role == "user":
            conversation_parts.append(f"User: {content}")

    system_prompt = "\n\n".join(system_parts) if system_parts else None

    # Si un seul message user sans historique, envoyer directement
    user_messages = [m for m in messages if m.get("role") == "user"]
    non_system = [m for m in messages if m.get("role") != "system"]

    if len(non_system) == 1 and non_system[0].get("role") == "user":
        content = non_system[0].get("content", "")
        if isinstance(content, list):
            text_parts = []
            for part in content:
                if isinstance(part, dict) and part.get("type") == "text":
                    text_parts.append(part["text"])
            content = "\n".join(text_parts)
        user_prompt = content
    else:
        # Multi-tour : formater la conversation
        user_prompt = "\n\n".join(conversation_parts)

    return system_prompt, user_prompt


def resolve_model(model_name):
    """Résout le nom de modèle vers un modèle Claude."""
    if not model_name:
        return DEFAULT_MODEL
    return MODEL_MAP.get(model_name, DEFAULT_MODEL)


def call_claude_cli(prompt, system_prompt=None, model=None, json_mode=False):
    """
    Appelle la CLI claude en mode pipe et retourne la réponse.

    Args:
        prompt: Le prompt utilisateur
        system_prompt: Le system prompt optionnel
        model: Le modèle Claude à utiliser
        json_mode: Si True, ajoute une instruction JSON au system prompt

    Returns:
        dict avec les clés: content, model, usage
    """
    model = resolve_model(model)

    # Construire le system prompt final
    final_system = system_prompt or ""
    if json_mode:
        json_instruction = (
            "\n\nIMPORTANT: You MUST respond with valid JSON only. "
            "No markdown code fences, no explanation, no text before or after. "
            "Just a raw JSON object or array."
        )
        final_system = (final_system + json_instruction).strip()

    # Construire la commande
    cmd = ["claude", "-p", "--output-format", "json", "--model", model]

    if final_system:
        cmd.extend(["--system-prompt", final_system])

    # Désactiver les outils MCP pour éviter les effets de bord
    cmd.extend(["--tools", ""])

    logger.info(f"Appel Claude CLI: model={model}, json_mode={json_mode}, prompt_len={len(prompt)}")

    try:
        result = subprocess.run(
            cmd,
            input=prompt,
            capture_output=True,
            text=True,
            timeout=DEFAULT_TIMEOUT,
        )

        if result.returncode != 0:
            error_msg = result.stderr.strip() or f"claude CLI exited with code {result.returncode}"
            logger.error(f"Claude CLI error: {error_msg}")
            raise RuntimeError(f"Claude CLI error: {error_msg}")

        # Parser la réponse JSON
        try:
            response = json.loads(result.stdout)
        except json.JSONDecodeError:
            # Si la sortie n'est pas du JSON, utiliser le texte brut
            logger.warning("Claude CLI output is not JSON, using raw text")
            response = {"result": result.stdout.strip()}

        content = response.get("result", "")
        usage = response.get("usage", {})

        # Si json_mode, valider et nettoyer le JSON
        if json_mode:
            content = clean_json_response(content)

        return {
            "content": content,
            "model": response.get("model", model),
            "usage": {
                "prompt_tokens": usage.get("input_tokens", 0),
                "completion_tokens": usage.get("output_tokens", 0),
                "total_tokens": usage.get("input_tokens", 0) + usage.get("output_tokens", 0),
            },
        }

    except subprocess.TimeoutExpired:
        logger.error(f"Claude CLI timeout after {DEFAULT_TIMEOUT}s")
        raise RuntimeError(f"Claude CLI timeout after {DEFAULT_TIMEOUT}s")
    except FileNotFoundError:
        logger.error("Claude CLI not found. Is 'claude' installed and in PATH?")
        raise RuntimeError(
            "Claude CLI not found. Install it with: npm install -g @anthropic-ai/claude-code"
        )


def clean_json_response(content):
    """
    Nettoie une réponse pour extraire du JSON valide.
    Gère les cas où Claude entoure le JSON de markdown.
    """
    content = content.strip()

    # Essayer de parser directement
    try:
        json.loads(content)
        return content
    except json.JSONDecodeError:
        pass

    # Extraire d'un bloc markdown ```json ... ```
    match = re.search(r"```(?:json)?\s*\n?(.*?)\n?\s*```", content, re.DOTALL)
    if match:
        extracted = match.group(1).strip()
        try:
            json.loads(extracted)
            return extracted
        except json.JSONDecodeError:
            pass

    # Chercher le premier { ou [ et le dernier } ou ]
    for start_char, end_char in [("{", "}"), ("[", "]")]:
        start = content.find(start_char)
        end = content.rfind(end_char)
        if start != -1 and end != -1 and end > start:
            extracted = content[start : end + 1]
            try:
                json.loads(extracted)
                return extracted
            except json.JSONDecodeError:
                continue

    # Retourner tel quel si rien ne marche
    logger.warning("Could not extract valid JSON from response")
    return content


def chat_completion(messages, model=None, json_mode=False):
    """
    Point d'entrée principal — traduit un appel OpenAI chat completion
    en appel Claude CLI.

    Returns:
        dict au format OpenAI chat completion response
    """
    system_prompt, user_prompt = format_messages_to_prompt(messages)
    result = call_claude_cli(
        prompt=user_prompt,
        system_prompt=system_prompt,
        model=model,
        json_mode=json_mode,
    )

    return {
        "id": f"chatcmpl-{uuid.uuid4().hex[:12]}",
        "object": "chat.completion",
        "model": result["model"],
        "choices": [
            {
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": result["content"],
                },
                "finish_reason": "stop",
            }
        ],
        "usage": result["usage"],
    }
