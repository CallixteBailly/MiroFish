//! Graph builder service — orchestrates knowledge graph construction.
//!
//! Supports two modes:
//! - **Local**: Chunk text, call LLM to extract entities/relations, store in SQLite.
//! - **Zep**: Upload text chunks to Zep Cloud for automatic graph extraction.

use serde_json::Value;
use std::sync::Arc;

use crate::llm::client::{ChatMessage, LlmClient};
use crate::models::task::{TaskManager, TaskStatus};
use crate::text::processor::TextProcessor;

use super::local::LocalGraphService;
use super::zep::ZepClient;

/// Graph info returned after building.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GraphBuildResult {
    pub graph_id: String,
    pub node_count: usize,
    pub edge_count: usize,
    pub entity_types: Vec<String>,
    pub chunks_processed: usize,
}

/// Graph builder service.
pub struct GraphBuilderService;

impl GraphBuilderService {
    /// Build a graph asynchronously using the local SQLite backend.
    ///
    /// Spawns a background task and returns the task ID immediately.
    pub fn build_local_async(
        llm: LlmClient,
        local: Arc<LocalGraphService>,
        task_manager: &TaskManager,
        text: String,
        ontology: Value,
        graph_name: String,
        chunk_size: usize,
        chunk_overlap: usize,
    ) -> String {
        let task_id = task_manager.create_task(
            "graph_build",
            Some(serde_json::json!({
                "graph_name": graph_name,
                "chunk_size": chunk_size,
                "text_length": text.len(),
                "mode": "local",
            })),
        );

        let tid = task_id.clone();
        let tm = TaskManager::global().clone();

        tokio::spawn(async move {
            if let Err(e) = Self::build_local_worker(&llm, &local, &tm, &tid, &text, &ontology, &graph_name, chunk_size, chunk_overlap).await {
                tracing::error!("Local graph build failed: {}", e);
                tm.fail_task(&tid, &e.to_string());
            }
        });

        task_id
    }

    /// Worker that builds the local graph.
    async fn build_local_worker(
        llm: &LlmClient,
        local: &LocalGraphService,
        tm: &TaskManager,
        task_id: &str,
        text: &str,
        ontology: &Value,
        graph_name: &str,
        chunk_size: usize,
        chunk_overlap: usize,
    ) -> anyhow::Result<()> {
        tm.update_task(task_id, Some(TaskStatus::Processing), Some(5), Some("Starting graph build...".to_string()), None, None, None);

        // 1. Create graph
        let graph_id = local.create_graph(graph_name)?;
        tm.update_task(task_id, None, Some(10), Some(format!("Graph created: {}", graph_id)), None, None, None);

        // 2. Set ontology
        local.set_ontology(&graph_id, ontology)?;
        tm.update_task(task_id, None, Some(15), Some("Ontology set".to_string()), None, None, None);

        // 3. Chunk text
        let chunks = TextProcessor::split_text(text, chunk_size, chunk_overlap);
        let total_chunks = chunks.len();
        tm.update_task(task_id, None, Some(20), Some(format!("Text split into {} chunks", total_chunks)), None, None, None);

        // 4. Extract entities and relations from each chunk
        for (i, chunk) in chunks.iter().enumerate() {
            let progress = 20 + ((i as u8) * 60 / total_chunks.max(1) as u8).min(60);
            tm.update_task(task_id, None, Some(progress), Some(format!("Processing chunk {}/{}", i + 1, total_chunks)), None, None, None);

            if let Err(e) = Self::extract_and_store(llm, local, &graph_id, chunk, ontology).await {
                tracing::warn!("Failed to process chunk {}: {}", i + 1, e);
            }
        }

        // 5. Get statistics
        tm.update_task(task_id, None, Some(90), Some("Retrieving graph info...".to_string()), None, None, None);
        let info = local.get_statistics(&graph_id)?;

        let result = serde_json::json!({
            "graph_id": graph_id,
            "graph_info": {
                "graph_id": info.graph_id,
                "node_count": info.node_count,
                "edge_count": info.edge_count,
                "entity_types": info.entity_types,
            },
            "chunks_processed": total_chunks,
        });

        tm.complete_task(task_id, result);
        Ok(())
    }

    /// Use LLM to extract entities and relations from a text chunk,
    /// then store them in the local graph.
    async fn extract_and_store(
        llm: &LlmClient,
        local: &LocalGraphService,
        graph_id: &str,
        chunk: &str,
        ontology: &Value,
    ) -> anyhow::Result<()> {
        // Build entity type names from ontology for the prompt
        let entity_type_names: Vec<String> = ontology
            .get("entity_types")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|e| e.get("name").and_then(|n| n.as_str()).map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        let edge_type_names: Vec<String> = ontology
            .get("edge_types")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|e| e.get("name").and_then(|n| n.as_str()).map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        let prompt = format!(
            "Extract entities and relationships from the following text.\n\
             \n\
             Entity types: {}\n\
             Relationship types: {}\n\
             \n\
             Text:\n{}\n\
             \n\
             Output JSON with:\n\
             {{\"entities\": [{{\"name\": \"...\", \"type\": \"...\", \"summary\": \"...\"}}],\n\
              \"relations\": [{{\"source\": \"...\", \"target\": \"...\", \"type\": \"...\", \"fact\": \"...\"}}]}}",
            entity_type_names.join(", "),
            edge_type_names.join(", "),
            chunk,
        );

        let messages = vec![ChatMessage::user(prompt)];
        let result = llm.chat_json(&messages, 0.2, Some(4096)).await;

        let data = match result {
            Ok(d) => d,
            Err(e) => {
                tracing::warn!("LLM extraction failed: {}", e);
                return Ok(());
            }
        };

        // Add entities
        if let Some(entities) = data.get("entities").and_then(|v| v.as_array()) {
            for entity in entities {
                let name = entity.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let etype = entity.get("type").and_then(|v| v.as_str()).unwrap_or("Entity");
                let summary = entity.get("summary").and_then(|v| v.as_str()).unwrap_or("");
                if !name.is_empty() {
                    let labels = vec!["Entity".to_string(), etype.to_string()];
                    local.add_node(graph_id, name, &labels, summary, None)?;
                }
            }
        }

        // Add relations
        if let Some(relations) = data.get("relations").and_then(|v| v.as_array()) {
            for rel in relations {
                let source = rel.get("source").and_then(|v| v.as_str()).unwrap_or("");
                let target = rel.get("target").and_then(|v| v.as_str()).unwrap_or("");
                let rtype = rel.get("type").and_then(|v| v.as_str()).unwrap_or("");
                let fact = rel.get("fact").and_then(|v| v.as_str()).unwrap_or("");
                if !source.is_empty() && !target.is_empty() {
                    local.add_edge(graph_id, source, target, rtype, fact)?;
                }
            }
        }

        Ok(())
    }

    /// Build a graph using Zep Cloud (async, returns task ID).
    pub fn build_zep_async(
        zep: ZepClient,
        task_manager: &TaskManager,
        text: String,
        ontology: Value,
        graph_name: String,
        chunk_size: usize,
        chunk_overlap: usize,
        batch_size: usize,
    ) -> String {
        let task_id = task_manager.create_task(
            "graph_build",
            Some(serde_json::json!({
                "graph_name": graph_name,
                "chunk_size": chunk_size,
                "text_length": text.len(),
                "mode": "zep",
            })),
        );

        let tid = task_id.clone();
        let tm = TaskManager::global().clone();

        tokio::spawn(async move {
            if let Err(e) = Self::build_zep_worker(&zep, &tm, &tid, &text, &ontology, &graph_name, chunk_size, chunk_overlap, batch_size).await {
                tracing::error!("Zep graph build failed: {}", e);
                tm.fail_task(&tid, &e.to_string());
            }
        });

        task_id
    }

    /// Worker that builds the Zep graph.
    async fn build_zep_worker(
        zep: &ZepClient,
        tm: &TaskManager,
        task_id: &str,
        text: &str,
        ontology: &Value,
        graph_name: &str,
        chunk_size: usize,
        chunk_overlap: usize,
        batch_size: usize,
    ) -> anyhow::Result<()> {
        tm.update_task(task_id, Some(TaskStatus::Processing), Some(5), Some("Starting graph build...".to_string()), None, None, None);

        // 1. Create graph
        let graph_id = zep.create_graph(graph_name).await?;
        tm.update_task(task_id, None, Some(10), Some(format!("Graph created: {}", graph_id)), None, None, None);

        // 2. Set ontology
        zep.set_ontology(&graph_id, ontology).await?;
        tm.update_task(task_id, None, Some(15), Some("Ontology set".to_string()), None, None, None);

        // 3. Chunk text
        let chunks = TextProcessor::split_text(text, chunk_size, chunk_overlap);
        let total_chunks = chunks.len();
        tm.update_task(task_id, None, Some(20), Some(format!("Text split into {} chunks", total_chunks)), None, None, None);

        // 4. Send batches
        let mut episode_uuids = Vec::new();
        let total_batches = (total_chunks + batch_size - 1) / batch_size;

        for (batch_idx, batch) in chunks.chunks(batch_size).enumerate() {
            let batch_num = batch_idx + 1;
            let progress = 20 + (batch_num * 40 / total_batches.max(1)).min(40) as u8;
            tm.update_task(task_id, None, Some(progress), Some(format!("Sending batch {}/{}", batch_num, total_batches)), None, None, None);

            let batch_texts: Vec<String> = batch.to_vec();
            let uuids = zep.add_batch(&graph_id, &batch_texts).await?;
            episode_uuids.extend(uuids);

            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        }

        // 5. Wait for processing
        tm.update_task(task_id, None, Some(60), Some("Waiting for Zep to process data...".to_string()), None, None, None);

        let timeout = std::time::Duration::from_secs(600);
        let start = std::time::Instant::now();
        let mut completed = 0usize;
        let total = episode_uuids.len();
        let mut pending: std::collections::HashSet<String> = episode_uuids.into_iter().collect();

        while !pending.is_empty() && start.elapsed() < timeout {
            let mut newly_done = Vec::new();
            for uuid in &pending {
                if zep.is_episode_processed(uuid).await.unwrap_or(false) {
                    newly_done.push(uuid.clone());
                }
            }
            for uuid in newly_done {
                pending.remove(&uuid);
                completed += 1;
            }

            let progress = 60 + (completed * 30 / total.max(1)).min(30) as u8;
            tm.update_task(task_id, None, Some(progress), Some(format!("Zep processing... {}/{} complete", completed, total)), None, None, None);

            if !pending.is_empty() {
                tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
            }
        }

        // 6. Get info
        tm.update_task(task_id, None, Some(90), Some("Retrieving graph info...".to_string()), None, None, None);

        let nodes = zep.get_nodes(&graph_id).await?;
        let edges = zep.get_edges(&graph_id).await?;

        let mut entity_types = std::collections::HashSet::new();
        for node in &nodes {
            if let Some(labels) = node.get("labels").and_then(|v| v.as_array()) {
                for label in labels {
                    if let Some(l) = label.as_str() {
                        if l != "Entity" && l != "Node" {
                            entity_types.insert(l.to_string());
                        }
                    }
                }
            }
        }

        let result = serde_json::json!({
            "graph_id": graph_id,
            "graph_info": {
                "graph_id": graph_id,
                "node_count": nodes.len(),
                "edge_count": edges.len(),
                "entity_types": entity_types.into_iter().collect::<Vec<_>>(),
            },
            "chunks_processed": total_chunks,
        });

        tm.complete_task(task_id, result);
        Ok(())
    }
}
