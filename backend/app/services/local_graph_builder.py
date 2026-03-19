"""
Local Graph Builder
Builds a knowledge graph from text using LLM entity/relationship extraction.
Stores results in LocalGraphService (SQLite + NetworkX).
"""

import json
import re
from typing import Callable, Optional

from openai import OpenAI

from ..config import Config
from ..utils.logger import get_logger
from .text_processor import TextProcessor
from .local_graph import LocalGraphService

logger = get_logger('mirofish.local_graph_builder')

MAX_ENTITIES_PER_CHUNK = 8
MAX_RELATIONS_PER_CHUNK = 10


class LocalGraphBuilder:
    """Builds a local knowledge graph from text using LLM extraction."""

    def __init__(self):
        self.client = OpenAI(api_key=Config.LLM_API_KEY, base_url=Config.LLM_BASE_URL)
        self.model = Config.LLM_MODEL_NAME
        self.graph = LocalGraphService()

    def build(
        self,
        graph_id: str,
        text: str,
        ontology: dict,
        chunk_size: int = 1500,
        chunk_overlap: int = 150,
        progress_callback: Optional[Callable[[int, int, str], None]] = None,
    ):
        """
        Build the graph from text.

        progress_callback(current, total, message)
        """
        self.graph.set_ontology(graph_id, ontology)

        chunks = TextProcessor.split_text(text, chunk_size=chunk_size, overlap=chunk_overlap)
        total = len(chunks)
        logger.info(f"Building local graph {graph_id}: {total} chunks")

        for i, chunk in enumerate(chunks):
            if progress_callback:
                progress_callback(i, total, f"Analyzing chunk {i + 1}/{total}...")
            try:
                self._process_chunk(graph_id, chunk, ontology)
            except Exception as e:
                logger.warning(f"Chunk {i + 1}/{total} failed: {e}")

        stats = self.graph.get_statistics(graph_id)
        msg = f"Done: {stats['node_count']} nodes, {stats['edge_count']} edges"
        if progress_callback:
            progress_callback(total, total, msg)
        logger.info(f"Local graph built: {graph_id} — {msg}")

    def _process_chunk(self, graph_id: str, chunk: str, ontology: dict):
        """Extract entities and relationships from one text chunk via LLM."""
        entity_types = [et["name"] for et in ontology.get("entity_types", [])]
        relation_types = [rt["name"] for rt in ontology.get("relationship_types", [])]

        prompt = (
            "Extract entities and relationships from the text below.\n\n"
            f"Entity types: {', '.join(entity_types)}\n"
            f"Relationship types: {', '.join(relation_types)}\n\n"
            f"Text:\n{chunk}\n\n"
            "Return JSON:\n"
            '{"entities": [{"name": "...", "type": "...", "description": "1-2 sentences"}], '
            '"relationships": [{"source": "entity name", "target": "entity name", "type": "RELATION_TYPE", "fact": "short fact"}]}\n'
            f"Max {MAX_ENTITIES_PER_CHUNK} entities, {MAX_RELATIONS_PER_CHUNK} relationships. "
            "Only include entities explicitly mentioned. Return only the JSON."
        )

        response = self.client.chat.completions.create(
            model=self.model,
            messages=[{"role": "user", "content": prompt}],
        )
        content = response.choices[0].message.content.strip()

        match = re.search(r'\{.*\}', content, re.DOTALL)
        if not match:
            return

        data = json.loads(match.group(0))

        for entity in data.get("entities", []):
            name = entity.get("name", "").strip()
            etype = entity.get("type", "Entity").strip()
            desc = entity.get("description", "")
            if name and etype:
                self.graph.add_node(graph_id, name, ["Entity", etype], desc)

        for rel in data.get("relationships", []):
            source = rel.get("source", "").strip()
            target = rel.get("target", "").strip()
            rtype = rel.get("type", "RELATED_TO").strip()
            fact = rel.get("fact", "")
            if source and target and rtype:
                self.graph.add_edge(graph_id, source, target, rtype, fact)
