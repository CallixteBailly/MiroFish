"""
Local Knowledge Graph Service
NetworkX + SQLite replacement for Zep Cloud.

Stores entity nodes and relationship edges locally.
Provides keyword search for report generation.
"""

import json
import sqlite3
import uuid
from contextlib import contextmanager
from pathlib import Path
from typing import Any, Dict, List, Optional, Set

from ..config import Config
from ..utils.logger import get_logger
from .zep_entity_reader import EntityNode, FilteredEntities

logger = get_logger('mirofish.local_graph')


def _db_path() -> Path:
    base = Path(Config.UPLOAD_FOLDER).parent
    return base / "local_graph.db"


class LocalGraphService:
    """NetworkX + SQLite knowledge graph, replaces Zep Cloud."""

    def __init__(self):
        self.db_path = _db_path()
        self._init_db()

    @contextmanager
    def _conn(self):
        conn = sqlite3.connect(self.db_path)
        conn.row_factory = sqlite3.Row
        try:
            yield conn
            conn.commit()
        finally:
            conn.close()

    def _init_db(self):
        with self._conn() as conn:
            conn.executescript("""
                CREATE TABLE IF NOT EXISTS graphs (
                    graph_id TEXT PRIMARY KEY,
                    name     TEXT,
                    ontology TEXT,
                    created_at TEXT DEFAULT (datetime('now'))
                );
                CREATE TABLE IF NOT EXISTS nodes (
                    uuid       TEXT PRIMARY KEY,
                    graph_id   TEXT,
                    name       TEXT,
                    labels     TEXT,
                    summary    TEXT,
                    attributes TEXT DEFAULT '{}',
                    created_at TEXT DEFAULT (datetime('now'))
                );
                CREATE TABLE IF NOT EXISTS edges (
                    uuid          TEXT PRIMARY KEY,
                    graph_id      TEXT,
                    source_uuid   TEXT,
                    target_uuid   TEXT,
                    source_name   TEXT,
                    target_name   TEXT,
                    relation_type TEXT,
                    fact          TEXT,
                    created_at    TEXT DEFAULT (datetime('now'))
                );
                CREATE INDEX IF NOT EXISTS idx_nodes_graph ON nodes(graph_id);
                CREATE INDEX IF NOT EXISTS idx_edges_graph ON edges(graph_id);
            """)

    # ──────────────────────────────── write ────────────────────────────────

    def create_graph(self, name: str) -> str:
        graph_id = f"local_{uuid.uuid4().hex[:12]}"
        with self._conn() as conn:
            conn.execute("INSERT INTO graphs (graph_id, name) VALUES (?, ?)", (graph_id, name))
        logger.info(f"Created local graph: {graph_id} ({name})")
        return graph_id

    def set_ontology(self, graph_id: str, ontology: dict):
        with self._conn() as conn:
            conn.execute(
                "UPDATE graphs SET ontology = ? WHERE graph_id = ?",
                (json.dumps(ontology, ensure_ascii=False), graph_id),
            )

    def add_node(self, graph_id: str, name: str, labels: List[str], summary: str, attributes: dict = None) -> str:
        with self._conn() as conn:
            row = conn.execute(
                "SELECT uuid FROM nodes WHERE graph_id = ? AND LOWER(name) = LOWER(?)",
                (graph_id, name),
            ).fetchone()
            if row:
                if summary:
                    conn.execute("UPDATE nodes SET summary = ? WHERE uuid = ?", (summary, row["uuid"]))
                return row["uuid"]
            node_uuid = str(uuid.uuid4())
            conn.execute(
                "INSERT INTO nodes (uuid, graph_id, name, labels, summary, attributes) VALUES (?, ?, ?, ?, ?, ?)",
                (node_uuid, graph_id, name, json.dumps(labels), summary, json.dumps(attributes or {})),
            )
            return node_uuid

    def add_edge(self, graph_id: str, source_name: str, target_name: str, relation_type: str, fact: str) -> Optional[str]:
        with self._conn() as conn:
            src = conn.execute(
                "SELECT uuid FROM nodes WHERE graph_id = ? AND LOWER(name) = LOWER(?)", (graph_id, source_name)
            ).fetchone()
            tgt = conn.execute(
                "SELECT uuid FROM nodes WHERE graph_id = ? AND LOWER(name) = LOWER(?)", (graph_id, target_name)
            ).fetchone()
            if not src or not tgt:
                return None
            dup = conn.execute(
                "SELECT uuid FROM edges WHERE graph_id=? AND source_uuid=? AND target_uuid=? AND relation_type=?",
                (graph_id, src["uuid"], tgt["uuid"], relation_type),
            ).fetchone()
            if dup:
                return dup["uuid"]
            edge_uuid = str(uuid.uuid4())
            conn.execute(
                "INSERT INTO edges (uuid, graph_id, source_uuid, target_uuid, source_name, target_name, relation_type, fact) "
                "VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                (edge_uuid, graph_id, src["uuid"], tgt["uuid"], source_name, target_name, relation_type, fact),
            )
            return edge_uuid

    # ──────────────────────────────── read ─────────────────────────────────

    def _row_to_entity(self, row, edges_by_node: dict) -> EntityNode:
        labels = json.loads(row["labels"])
        related = [
            {"relation_type": e["relation_type"], "fact": e["fact"],
             "source": e["source_name"], "target": e["target_name"]}
            for e in edges_by_node.get(row["uuid"], [])
        ]
        return EntityNode(
            uuid=row["uuid"],
            name=row["name"],
            labels=labels,
            summary=row["summary"] or "",
            attributes=json.loads(row["attributes"]),
            related_edges=related,
            related_nodes=[],
        )

    def get_nodes(self, graph_id: str, entity_types: List[str] = None) -> List[EntityNode]:
        with self._conn() as conn:
            node_rows = conn.execute("SELECT * FROM nodes WHERE graph_id = ?", (graph_id,)).fetchall()
            edge_rows = conn.execute("SELECT * FROM edges WHERE graph_id = ?", (graph_id,)).fetchall()

        edges_by_node: Dict[str, list] = {}
        for e in edge_rows:
            edges_by_node.setdefault(e["source_uuid"], []).append(dict(e))
            edges_by_node.setdefault(e["target_uuid"], []).append(dict(e))

        nodes = []
        for row in node_rows:
            labels = json.loads(row["labels"])
            if entity_types:
                etype = next((l for l in labels if l not in ("Entity", "Node")), None)
                if etype not in entity_types:
                    continue
            nodes.append(self._row_to_entity(dict(row), edges_by_node))
        return nodes

    def get_filtered_entities(self, graph_id: str, entity_types: List[str] = None) -> FilteredEntities:
        nodes = self.get_nodes(graph_id, entity_types)
        type_set: Set[str] = {n.get_entity_type() for n in nodes if n.get_entity_type()}
        return FilteredEntities(entities=nodes, entity_types=type_set, total_count=len(nodes), filtered_count=len(nodes))

    def get_all_edges(self, graph_id: str) -> List[dict]:
        with self._conn() as conn:
            return [dict(r) for r in conn.execute("SELECT * FROM edges WHERE graph_id = ?", (graph_id,)).fetchall()]

    def get_statistics(self, graph_id: str) -> dict:
        with self._conn() as conn:
            nc = conn.execute("SELECT COUNT(*) FROM nodes WHERE graph_id = ?", (graph_id,)).fetchone()[0]
            ec = conn.execute("SELECT COUNT(*) FROM edges WHERE graph_id = ?", (graph_id,)).fetchone()[0]
            g = conn.execute("SELECT name FROM graphs WHERE graph_id = ?", (graph_id,)).fetchone()
        nodes = self.get_nodes(graph_id)
        type_counts: Dict[str, int] = {}
        for n in nodes:
            et = n.get_entity_type()
            if et:
                type_counts[et] = type_counts.get(et, 0) + 1
        return {"graph_id": graph_id, "name": g["name"] if g else "", "node_count": nc, "edge_count": ec, "entity_types": type_counts}

    def graph_exists(self, graph_id: str) -> bool:
        with self._conn() as conn:
            return conn.execute("SELECT 1 FROM graphs WHERE graph_id = ?", (graph_id,)).fetchone() is not None

    def get_graph_data_for_api(self, graph_id: str) -> dict:
        stats = self.get_statistics(graph_id)
        nodes = self.get_nodes(graph_id)
        edges = self.get_all_edges(graph_id)
        return {
            "graph_id": graph_id,
            "node_count": stats["node_count"],
            "edge_count": stats["edge_count"],
            "nodes": [n.to_dict() for n in nodes],
            "edges": edges,
            "entity_types": list(stats["entity_types"].keys()),
            "local_graph": True,
        }

    # ─────────────────────────────── search ────────────────────────────────

    def search(self, graph_id: str, query: str, limit: int = 10) -> List[dict]:
        """Keyword search across nodes and edges."""
        words = query.lower().split()
        results = []

        for node in self.get_nodes(graph_id):
            text = f"{node.name} {node.summary}".lower()
            score = sum(text.count(w) for w in words if w in text)
            if score:
                results.append({"type": "node", "score": score, "name": node.name, "summary": node.summary, "labels": node.labels})

        for edge in self.get_all_edges(graph_id):
            text = f"{edge['source_name']} {edge['target_name']} {edge['relation_type']} {edge.get('fact', '')}".lower()
            score = sum(text.count(w) for w in words if w in text)
            if score:
                results.append({"type": "edge", "score": score, **edge})

        results.sort(key=lambda x: x["score"], reverse=True)
        return results[:limit]
