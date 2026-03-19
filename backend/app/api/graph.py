"""
Graph-related API routes
Uses project context mechanism with server-side persistent state
"""

import os
import traceback
import threading
from flask import request, jsonify

from . import graph_bp
from ..config import Config
from ..services.ontology_generator import OntologyGenerator
from ..services.graph_builder import GraphBuilderService
from ..services.local_graph import LocalGraphService
from ..services.local_graph_builder import LocalGraphBuilder
from ..services.text_processor import TextProcessor
from ..utils.file_parser import FileParser
from ..utils.logger import get_logger
from ..models.task import TaskManager, TaskStatus
from ..models.project import ProjectManager, ProjectStatus

# Get logger
logger = get_logger('mirofish.api')


def allowed_file(filename: str) -> bool:
    """Check if the file extension is allowed"""
    if not filename or '.' not in filename:
        return False
    ext = os.path.splitext(filename)[1].lower().lstrip('.')
    return ext in Config.ALLOWED_EXTENSIONS


# ============== Project Management Endpoints ==============

@graph_bp.route('/project/<project_id>', methods=['GET'])
def get_project(project_id: str):
    """
    Get project details
    """
    project = ProjectManager.get_project(project_id)

    if not project:
        return jsonify({
            "success": False,
            "error": f"Project not found: {project_id}"
        }), 404

    return jsonify({
        "success": True,
        "data": project.to_dict()
    })


@graph_bp.route('/project/list', methods=['GET'])
def list_projects():
    """
    List all projects
    """
    limit = request.args.get('limit', 50, type=int)
    projects = ProjectManager.list_projects(limit=limit)

    return jsonify({
        "success": True,
        "data": [p.to_dict() for p in projects],
        "count": len(projects)
    })


@graph_bp.route('/project/<project_id>', methods=['DELETE'])
def delete_project(project_id: str):
    """
    Delete a project
    """
    success = ProjectManager.delete_project(project_id)

    if not success:
        return jsonify({
            "success": False,
            "error": f"Project not found or deletion failed: {project_id}"
        }), 404

    return jsonify({
        "success": True,
        "message": f"Project deleted: {project_id}"
    })


@graph_bp.route('/project/<project_id>/reset', methods=['POST'])
def reset_project(project_id: str):
    """
    Reset project status (for rebuilding the graph)
    """
    project = ProjectManager.get_project(project_id)

    if not project:
        return jsonify({
            "success": False,
            "error": f"Project not found: {project_id}"
        }), 404

    # Reset to ontology-generated state
    if project.ontology:
        project.status = ProjectStatus.ONTOLOGY_GENERATED
    else:
        project.status = ProjectStatus.CREATED

    project.graph_id = None
    project.graph_build_task_id = None
    project.error = None
    ProjectManager.save_project(project)

    return jsonify({
        "success": True,
        "message": f"Project reset: {project_id}",
        "data": project.to_dict()
    })


# ============== Endpoint 1: Upload Files and Generate Ontology ==============

@graph_bp.route('/ontology/generate', methods=['POST'])
def generate_ontology():
    """
    Endpoint 1: Upload files and analyze to generate ontology definition

    Request method: multipart/form-data

    Parameters:
        files: Uploaded files (PDF/MD/TXT), multiple allowed
        simulation_requirement: Simulation requirement description (required)
        project_name: Project name (optional)
        additional_context: Additional notes (optional)

    Returns:
        {
            "success": true,
            "data": {
                "project_id": "proj_xxxx",
                "ontology": {
                    "entity_types": [...],
                    "edge_types": [...],
                    "analysis_summary": "..."
                },
                "files": [...],
                "total_text_length": 12345
            }
        }
    """
    try:
        logger.info("=== Starting ontology generation ===")

        # Get parameters
        simulation_requirement = request.form.get('simulation_requirement', '')
        project_name = request.form.get('project_name', 'Unnamed Project')
        additional_context = request.form.get('additional_context', '')

        logger.debug(f"Project name: {project_name}")
        logger.debug(f"Simulation requirement: {simulation_requirement[:100]}...")

        if not simulation_requirement:
            return jsonify({
                "success": False,
                "error": "Please provide simulation_requirement"
            }), 400

        # Get uploaded files
        uploaded_files = request.files.getlist('files')
        if not uploaded_files or all(not f.filename for f in uploaded_files):
            return jsonify({
                "success": False,
                "error": "Please upload at least one document file"
            }), 400

        # Create project
        project = ProjectManager.create_project(name=project_name)
        project.simulation_requirement = simulation_requirement
        logger.info(f"Created project: {project.project_id}")

        # Save files and extract text
        document_texts = []
        all_text = ""

        for file in uploaded_files:
            if file and file.filename and allowed_file(file.filename):
                # Save file to project directory
                file_info = ProjectManager.save_file_to_project(
                    project.project_id,
                    file,
                    file.filename
                )
                project.files.append({
                    "filename": file_info["original_filename"],
                    "size": file_info["size"]
                })

                # Extract text
                text = FileParser.extract_text(file_info["path"])
                text = TextProcessor.preprocess_text(text)
                document_texts.append(text)
                all_text += f"\n\n=== {file_info['original_filename']} ===\n{text}"

        if not document_texts:
            ProjectManager.delete_project(project.project_id)
            return jsonify({
                "success": False,
                "error": "No documents were successfully processed, please check the file formats"
            }), 400

        # Save extracted text
        project.total_text_length = len(all_text)
        ProjectManager.save_extracted_text(project.project_id, all_text)
        logger.info(f"Text extraction complete, {len(all_text)} characters total")

        # Generate ontology
        logger.info("Calling LLM to generate ontology definition...")
        generator = OntologyGenerator()
        ontology = generator.generate(
            document_texts=document_texts,
            simulation_requirement=simulation_requirement,
            additional_context=additional_context if additional_context else None
        )

        # Save ontology to project
        entity_count = len(ontology.get("entity_types", []))
        edge_count = len(ontology.get("edge_types", []))
        logger.info(f"Ontology generation complete: {entity_count} entity types, {edge_count} relationship types")

        project.ontology = {
            "entity_types": ontology.get("entity_types", []),
            "edge_types": ontology.get("edge_types", [])
        }
        project.analysis_summary = ontology.get("analysis_summary", "")
        project.status = ProjectStatus.ONTOLOGY_GENERATED
        ProjectManager.save_project(project)
        logger.info(f"=== Ontology generation complete === Project ID: {project.project_id}")

        return jsonify({
            "success": True,
            "data": {
                "project_id": project.project_id,
                "project_name": project.name,
                "ontology": project.ontology,
                "analysis_summary": project.analysis_summary,
                "files": project.files,
                "total_text_length": project.total_text_length
            }
        })

    except Exception as e:
        return jsonify({
            "success": False,
            "error": str(e),
            "traceback": traceback.format_exc()
        }), 500


# ============== Endpoint 2: Build Graph ==============

@graph_bp.route('/build', methods=['POST'])
def build_graph():
    """
    Endpoint 2: Build graph based on project_id

    Request (JSON):
        {
            "project_id": "proj_xxxx",  // required, from endpoint 1
            "graph_name": "Graph Name", // optional
            "chunk_size": 500,          // optional, default 500
            "chunk_overlap": 50         // optional, default 50
        }

    Returns:
        {
            "success": true,
            "data": {
                "project_id": "proj_xxxx",
                "task_id": "task_xxxx",
                "message": "Graph build task started"
            }
        }
    """
    try:
        logger.info("=== Starting graph build ===")

        graph_mode = Config.graph_mode()

        # Parse request (common to all modes)
        data = request.get_json() or {}
        project_id = data.get('project_id')
        if not project_id:
            return jsonify({"success": False, "error": "Please provide project_id"}), 400
        project = ProjectManager.get_project(project_id)
        if not project:
            return jsonify({"success": False, "error": f"Project not found: {project_id}"}), 404

        # ── NONE mode: no graph, skip immediately ──────────────────────────
        if graph_mode == "none":
            project.graph_id = "lite_mode"
            project.status = ProjectStatus.GRAPH_COMPLETED
            ProjectManager.save_project(project)
            task_manager = TaskManager()
            task_id = task_manager.create_task(f"Build graph (none): {project.name or project_id}")
            task_manager.complete_task(task_id, result={"graph_id": "lite_mode"})
            logger.info(f"NONE mode: graph build skipped, project={project_id}")
            return jsonify({"success": True, "data": {"task_id": task_id, "project_id": project_id, "graph_id": "lite_mode"}})

        # ── Check project state ────────────────────────────────────────────
        force = data.get('force', False)

        if project.status == ProjectStatus.CREATED:
            return jsonify({"success": False, "error": "Ontology not generated yet, call /ontology/generate first"}), 400

        if project.status == ProjectStatus.GRAPH_BUILDING and not force:
            return jsonify({"success": False, "error": "Graph is already building. Add force:true to rebuild.", "task_id": project.graph_build_task_id}), 400

        if force and project.status in [ProjectStatus.GRAPH_BUILDING, ProjectStatus.FAILED, ProjectStatus.GRAPH_COMPLETED]:
            project.status = ProjectStatus.ONTOLOGY_GENERATED
            project.graph_id = None
            project.graph_build_task_id = None
            project.error = None

        graph_name = data.get('graph_name', project.name or 'MiroFish Graph')
        chunk_size = data.get('chunk_size', project.chunk_size or Config.DEFAULT_CHUNK_SIZE)
        chunk_overlap = data.get('chunk_overlap', project.chunk_overlap or Config.DEFAULT_CHUNK_OVERLAP)
        project.chunk_size = chunk_size
        project.chunk_overlap = chunk_overlap

        text = ProjectManager.get_extracted_text(project_id)
        if not text:
            return jsonify({"success": False, "error": "Extracted text content not found"}), 400
        ontology = project.ontology
        if not ontology:
            return jsonify({"success": False, "error": "Ontology definition not found"}), 400

        task_manager = TaskManager()
        task_id = task_manager.create_task(f"Build graph: {graph_name}")
        logger.info(f"Created graph build task: task_id={task_id}, project_id={project_id}, mode={graph_mode}")
        project.status = ProjectStatus.GRAPH_BUILDING
        project.graph_build_task_id = task_id
        ProjectManager.save_project(project)

        # ── LOCAL mode: NetworkX + SQLite ─────────────────────────────────
        if graph_mode == "local":
            def build_local():
                build_logger = get_logger('mirofish.build')
                try:
                    task_manager.update_task(task_id, status=TaskStatus.PROCESSING, message="Creating local graph...", progress=5)
                    local_builder = LocalGraphBuilder()
                    graph_id = local_builder.graph.create_graph(graph_name)
                    project.graph_id = graph_id
                    ProjectManager.save_project(project)

                    def cb(current, total, msg):
                        pct = 10 + int(current / max(total, 1) * 85)
                        task_manager.update_task(task_id, message=msg, progress=pct)

                    local_builder.build(graph_id, text, ontology, chunk_size, chunk_overlap, progress_callback=cb)
                    stats = local_builder.graph.get_statistics(graph_id)
                    project.status = ProjectStatus.GRAPH_COMPLETED
                    ProjectManager.save_project(project)
                    build_logger.info(f"[{task_id}] Local graph built: {stats['node_count']} nodes, {stats['edge_count']} edges")
                    task_manager.update_task(task_id, status=TaskStatus.COMPLETED,
                        message=f"Graph built: {stats['node_count']} nodes, {stats['edge_count']} edges",
                        progress=100, result={"graph_id": graph_id, **stats})
                except Exception as e:
                    build_logger.error(f"[{task_id}] Local graph build failed: {e}")
                    project.status = ProjectStatus.FAILED
                    project.error = str(e)
                    ProjectManager.save_project(project)
                    task_manager.update_task(task_id, status=TaskStatus.FAILED, message=f"Build failed: {e}", error=traceback.format_exc())

            threading.Thread(target=build_local, daemon=True).start()

        # ── ZEP mode: Zep Cloud ────────────────────────────────────────────
        else:
            def build_task():
                build_logger = get_logger('mirofish.build')
                try:
                    task_manager.update_task(task_id, status=TaskStatus.PROCESSING, message="Initializing graph build service...", progress=5)
                    builder = GraphBuilderService(api_key=Config.ZEP_API_KEY)
                    chunks = TextProcessor.split_text(text, chunk_size=chunk_size, overlap=chunk_overlap)
                    total_chunks = len(chunks)
                    task_manager.update_task(task_id, message="Creating Zep graph...", progress=10)
                    graph_id = builder.create_graph(name=graph_name)
                    project.graph_id = graph_id
                    ProjectManager.save_project(project)
                    builder.set_ontology(graph_id, ontology)

                    def add_cb(msg, ratio):
                        task_manager.update_task(task_id, message=msg, progress=15 + int(ratio * 40))

                    episode_uuids = builder.add_text_batches(graph_id, chunks, batch_size=3, progress_callback=add_cb)
                    task_manager.update_task(task_id, message="Waiting for Zep to process data...", progress=55)

                    def wait_cb(msg, ratio):
                        task_manager.update_task(task_id, message=msg, progress=55 + int(ratio * 35))

                    builder._wait_for_episodes(episode_uuids, wait_cb)
                    graph_data = builder.get_graph_data(graph_id)
                    project.status = ProjectStatus.GRAPH_COMPLETED
                    ProjectManager.save_project(project)
                    nc, ec = graph_data.get("node_count", 0), graph_data.get("edge_count", 0)
                    task_manager.update_task(task_id, status=TaskStatus.COMPLETED, message="Graph build complete",
                        progress=100, result={"project_id": project_id, "graph_id": graph_id, "node_count": nc, "edge_count": ec, "chunk_count": total_chunks})
                except Exception as e:
                    build_logger.error(f"[{task_id}] Graph build failed: {e}")
                    project.status = ProjectStatus.FAILED
                    project.error = str(e)
                    ProjectManager.save_project(project)
                    task_manager.update_task(task_id, status=TaskStatus.FAILED, message=f"Build failed: {e}", error=traceback.format_exc())

            threading.Thread(target=build_task, daemon=True).start()

        return jsonify({
            "success": True,
            "data": {
                "project_id": project_id,
                "task_id": task_id,
                "message": "Graph build task started, check progress via /task/{task_id}"
            }
        })

    except Exception as e:
        return jsonify({
            "success": False,
            "error": str(e),
            "traceback": traceback.format_exc()
        }), 500


# ============== Task Query Endpoints ==============

@graph_bp.route('/task/<task_id>', methods=['GET'])
def get_task(task_id: str):
    """
    Query task status
    """
    task = TaskManager().get_task(task_id)

    if not task:
        return jsonify({
            "success": False,
            "error": f"Task not found: {task_id}"
        }), 404

    return jsonify({
        "success": True,
        "data": task.to_dict()
    })


@graph_bp.route('/tasks', methods=['GET'])
def list_tasks():
    """
    List all tasks
    """
    tasks = TaskManager().list_tasks()

    return jsonify({
        "success": True,
        "data": [t.to_dict() for t in tasks],
        "count": len(tasks)
    })


# ============== Graph Data Endpoints ==============

@graph_bp.route('/data/<graph_id>', methods=['GET'])
def get_graph_data(graph_id: str):
    """
    Get graph data (nodes and edges)
    """
    try:
        if graph_id == "lite_mode":
            return jsonify({"success": True, "data": {"graph_id": "lite_mode", "node_count": 0, "edge_count": 0, "nodes": [], "edges": []}})

        if graph_id.startswith("local_"):
            local = LocalGraphService()
            return jsonify({"success": True, "data": local.get_graph_data_for_api(graph_id)})

        if not Config.is_zep_available():
            return jsonify({"success": False, "error": "Zep Cloud is not configured."}), 503

        builder = GraphBuilderService(api_key=Config.ZEP_API_KEY)
        graph_data = builder.get_graph_data(graph_id)

        return jsonify({
            "success": True,
            "data": graph_data
        })

    except Exception as e:
        return jsonify({
            "success": False,
            "error": str(e),
            "traceback": traceback.format_exc()
        }), 500


@graph_bp.route('/delete/<graph_id>', methods=['DELETE'])
def delete_graph(graph_id: str):
    """
    Delete a Zep graph
    """
    try:
        if not Config.ZEP_API_KEY:
            return jsonify({
                "success": False,
                "error": "ZEP_API_KEY is not configured. This feature requires Zep Cloud. Please set ZEP_API_KEY in your .env file.",
                "lite_mode": Config.LITE_MODE
            }), 503

        builder = GraphBuilderService(api_key=Config.ZEP_API_KEY)
        builder.delete_graph(graph_id)

        return jsonify({
            "success": True,
            "message": f"Graph deleted: {graph_id}"
        })

    except Exception as e:
        return jsonify({
            "success": False,
            "error": str(e),
            "traceback": traceback.format_exc()
        }), 500
