"""
Lite Entity Extractor
Uses LLM to extract entity instances from documents according to an ontology.
Used in LITE_MODE as a replacement for ZepEntityReader.
"""

import json
import re
import uuid
from typing import List

from openai import OpenAI

from ..config import Config
from ..utils.logger import get_logger
from .zep_entity_reader import EntityNode

logger = get_logger('mirofish.lite_entity_extractor')


class LiteEntityExtractor:
    """Extracts entity instances from documents via LLM, replacing ZepEntityReader in LITE_MODE."""

    MAX_DOC_CHARS = 8000

    def __init__(self):
        self.client = OpenAI(
            api_key=Config.LLM_API_KEY,
            base_url=Config.LLM_BASE_URL,
        )
        self.model = Config.LLM_MODEL_NAME

    def extract_entities(
        self,
        document_text: str,
        ontology: dict,
        max_entities: int = 25,
    ) -> List[EntityNode]:
        """
        Extract entity instances from document text according to the ontology.

        Returns:
            List of EntityNode instances
        """
        entity_types = ontology.get("entity_types", [])
        if not entity_types:
            logger.warning("No entity types in ontology, using generic extraction")
            entity_types = [{"name": "Person", "description": "A person mentioned in the document"}]

        type_descriptions = "\n".join(
            f"- {et['name']}: {et.get('description', '')}" for et in entity_types
        )

        doc_excerpt = document_text[:self.MAX_DOC_CHARS]

        prompt = (
            "Analyze the following document and extract specific entities (people, organizations, "
            "concepts, etc.) that could serve as social media simulation agents.\n\n"
            f"Entity types defined in the ontology:\n{type_descriptions}\n\n"
            f"Document content:\n{doc_excerpt}\n\n"
            f"Extract up to {max_entities} distinct entities. For each entity provide:\n"
            "- name: the entity name\n"
            "- type: one of the entity types above\n"
            "- summary: a 2-3 sentence description based on the document\n\n"
            "Return a JSON array:\n"
            '[{"name": "...", "type": "...", "summary": "..."}, ...]\n'
            "Only the JSON array, no other text."
        )

        try:
            response = self.client.chat.completions.create(
                model=self.model,
                messages=[{"role": "user", "content": prompt}],
            )
            content = response.choices[0].message.content.strip()

            # Extract JSON array even if wrapped in markdown
            match = re.search(r'\[.*\]', content, re.DOTALL)
            if match:
                content = match.group(0)

            entities_data = json.loads(content)
            if isinstance(entities_data, dict):
                entities_data = next(iter(entities_data.values()), [])

            entities = []
            for item in entities_data[:max_entities]:
                entities.append(EntityNode(
                    uuid=str(uuid.uuid4()),
                    name=item.get("name", "Unknown"),
                    labels=["Entity", item.get("type", "Person")],
                    summary=item.get("summary", ""),
                    attributes={},
                    related_edges=[],
                    related_nodes=[],
                ))

            logger.info(f"Extracted {len(entities)} entities from document (LITE_MODE)")
            return entities

        except Exception as e:
            logger.error(f"Entity extraction failed: {e}")
            return self._fallback_entities(entity_types)

    def _fallback_entities(self, entity_types: list) -> List[EntityNode]:
        """Return minimal fallback entities if extraction fails."""
        return [
            EntityNode(
                uuid=str(uuid.uuid4()),
                name=f"Sample {et['name']}",
                labels=["Entity", et["name"]],
                summary=et.get("description", f"A representative {et['name']}"),
                attributes={},
                related_edges=[],
                related_nodes=[],
            )
            for et in entity_types[:5]
        ]
