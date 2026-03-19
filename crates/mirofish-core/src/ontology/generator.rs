//! Ontology generation service.
//!
//! Analyzes text content and generates entity-type / edge-type definitions
//! suitable for social media opinion simulation.

use serde::{Deserialize, Serialize};

use crate::llm::client::{ChatMessage, LlmClient, LlmError};

/// System prompt for ontology generation (matches Python implementation).
const ONTOLOGY_SYSTEM_PROMPT: &str = r#"You are a professional knowledge graph ontology design expert. Your task is to analyze given text content and simulation requirements, and design entity types and relationship types suitable for **social media opinion simulation**.

**Important: You must output valid JSON format data. Do not output anything else.**

## Core Task Background

We are building a **social media opinion simulation system**. In this system:
- Each entity is an "account" or "subject" that can post, interact, and spread information on social media
- Entities mutually influence each other through reposts, comments, and responses
- We need to simulate each party's reactions and information propagation paths during opinion events

Therefore, **entities must be real-world subjects that can speak and interact on social media**:

**Can be**:
- Specific individuals (public figures, involved parties, opinion leaders, experts, ordinary people)
- Companies and enterprises (including their official accounts)
- Organizations (universities, associations, NGOs, trade unions, etc.)
- Government departments and regulatory agencies
- Media organizations (newspapers, TV stations, self-media, websites)
- Social media platforms themselves
- Representatives of specific groups (e.g., alumni associations, fan clubs, rights advocacy groups)

**Cannot be**:
- Abstract concepts (e.g., "public opinion", "sentiment", "trends")
- Topics/themes (e.g., "academic integrity", "education reform")
- Viewpoints/stances (e.g., "supporters", "opponents")

## Output Format

Please output JSON format with the following structure:

```json
{
    "entity_types": [
        {
            "name": "Entity type name (English, PascalCase)",
            "description": "Short description (English, max 100 characters)",
            "attributes": [
                {
                    "name": "Attribute name (English, snake_case)",
                    "type": "text",
                    "description": "Attribute description"
                }
            ],
            "examples": ["Example entity 1", "Example entity 2"]
        }
    ],
    "edge_types": [
        {
            "name": "Relationship type name (English, UPPER_SNAKE_CASE)",
            "description": "Short description (English, max 100 characters)",
            "source_targets": [
                {"source": "Source entity type", "target": "Target entity type"}
            ],
            "attributes": []
        }
    ],
    "analysis_summary": "Brief analysis summary of the text content (English)"
}
```

## Design Guidelines (Extremely Important!)

### 1. Entity Type Design - Must Strictly Follow

**Quantity requirement: Must be exactly 10 entity types**

**Hierarchy requirement (must include both specific types and fallback types)**:

Your 10 entity types must include the following hierarchy:

A. **Fallback types (required, placed as the last 2 in the list)**:
   - `Person`: Fallback type for any individual person. Used when a person does not fit any other more specific person type.
   - `Organization`: Fallback type for any organization. Used when an organization does not fit any other more specific organization type.

B. **Specific types (8 types, designed based on text content)**:
   - Design more specific types targeting the major roles appearing in the text
   - Example: if text involves an academic event, you could have `Student`, `Professor`, `University`
   - Example: if text involves a business event, you could have `Company`, `CEO`, `Employee`

**Why fallback types are needed**:
- Text will include various people, such as "primary school teachers", "passersby", "some netizen"
- If there is no specific type match, they should be classified as `Person`
- Similarly, small organizations and temporary groups should be classified as `Organization`

**Principles for designing specific types**:
- Identify high-frequency or key role types from the text
- Each specific type should have clear boundaries to avoid overlap
- The description must clearly explain how this type differs from the fallback type

### 2. Relationship Type Design

- Quantity: 6-10
- Relationships should reflect real connections in social media interactions
- Ensure relationship source_targets cover the entity types you have defined

### 3. Attribute Design

- 1-3 key attributes per entity type
- **Note**: Attribute names cannot use `name`, `uuid`, `group_id`, `created_at`, `summary` (these are system reserved words)
- Recommended: `full_name`, `title`, `role`, `position`, `location`, `description`, etc."#;

/// Maximum text length passed to LLM (50 000 characters).
const MAX_TEXT_LENGTH_FOR_LLM: usize = 50_000;
/// Max entity types (Zep API limit).
const MAX_ENTITY_TYPES: usize = 10;
/// Max edge types (Zep API limit).
const MAX_EDGE_TYPES: usize = 10;

/// An individual entity-type attribute.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OntologyAttribute {
    pub name: String,
    #[serde(default = "default_text")]
    pub r#type: String,
    #[serde(default)]
    pub description: String,
}

fn default_text() -> String { "text".to_string() }

/// A source-target pair for an edge type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceTarget {
    pub source: String,
    pub target: String,
}

/// An entity type definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityTypeDef {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub attributes: Vec<OntologyAttribute>,
    #[serde(default)]
    pub examples: Vec<String>,
}

/// An edge (relationship) type definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeTypeDef {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub source_targets: Vec<SourceTarget>,
    #[serde(default)]
    pub attributes: Vec<OntologyAttribute>,
}

/// Result of ontology generation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ontology {
    pub entity_types: Vec<EntityTypeDef>,
    pub edge_types: Vec<EdgeTypeDef>,
    #[serde(default)]
    pub analysis_summary: String,
}

/// Ontology generator — uses LLM to produce entity/edge type definitions.
pub struct OntologyGenerator {
    llm: LlmClient,
}

impl OntologyGenerator {
    /// Create a generator with the given LLM client.
    pub fn new(llm: LlmClient) -> Self {
        Self { llm }
    }

    /// Create a generator from the global config.
    pub fn from_global_config() -> Result<Self, LlmError> {
        Ok(Self { llm: LlmClient::from_global_config()? })
    }

    /// Generate an ontology definition from documents and simulation requirement.
    pub async fn generate(
        &self,
        document_texts: &[String],
        simulation_requirement: &str,
        additional_context: Option<&str>,
    ) -> Result<Ontology, LlmError> {
        let user_message = self.build_user_message(document_texts, simulation_requirement, additional_context);

        let messages = vec![
            ChatMessage::system(ONTOLOGY_SYSTEM_PROMPT),
            ChatMessage::user(user_message),
        ];

        let result = self.llm.chat_json(&messages, 0.3, Some(4096)).await?;

        // Parse and validate
        let mut ontology: Ontology = serde_json::from_value(result)
            .map_err(|e| LlmError::InvalidJson(e.to_string()))?;

        Self::validate_and_process(&mut ontology);

        Ok(ontology)
    }

    /// Build the user message including document content.
    fn build_user_message(
        &self,
        document_texts: &[String],
        simulation_requirement: &str,
        additional_context: Option<&str>,
    ) -> String {
        let mut combined = document_texts.join("\n\n---\n\n");
        let original_length = combined.chars().count();

        if combined.chars().count() > MAX_TEXT_LENGTH_FOR_LLM {
            combined = combined.chars().take(MAX_TEXT_LENGTH_FOR_LLM).collect();
            combined.push_str(&format!(
                "\n\n...(original text is {} characters; first {} characters used for ontology analysis)...",
                original_length, MAX_TEXT_LENGTH_FOR_LLM
            ));
        }

        let mut msg = format!(
            "## Simulation Requirement\n\n{}\n\n## Document Content\n\n{}\n",
            simulation_requirement, combined
        );

        if let Some(ctx) = additional_context {
            msg.push_str(&format!("\n## Additional Notes\n\n{}\n", ctx));
        }

        msg.push_str(
            "\nBased on the above content, please design entity types and relationship types suitable for social opinion simulation.\n\n\
             **Rules that must be followed**:\n\
             1. Must output exactly 10 entity types\n\
             2. The last 2 must be fallback types: Person (individual fallback) and Organization (organization fallback)\n\
             3. The first 8 are specific types designed based on the text content\n\
             4. All entity types must be real-world subjects capable of expressing opinions; they cannot be abstract concepts\n\
             5. Attribute names cannot use reserved words such as name, uuid, group_id; use full_name, org_name, etc. instead\n"
        );

        msg
    }

    /// Validate and post-process the generated ontology.
    fn validate_and_process(ontology: &mut Ontology) {
        // Ensure description lengths
        for e in ontology.entity_types.iter_mut() {
            if e.description.chars().count() > 100 {
                e.description = e.description.chars().take(97).collect::<String>() + "...";
            }
        }
        for e in ontology.edge_types.iter_mut() {
            if e.description.chars().count() > 100 {
                e.description = e.description.chars().take(97).collect::<String>() + "...";
            }
        }

        // Ensure fallback types exist
        let has_person = ontology.entity_types.iter().any(|e| e.name == "Person");
        let has_org = ontology.entity_types.iter().any(|e| e.name == "Organization");

        let mut fallbacks: Vec<EntityTypeDef> = Vec::new();

        if !has_person {
            fallbacks.push(EntityTypeDef {
                name: "Person".to_string(),
                description: "Any individual person not fitting other specific person types.".to_string(),
                attributes: vec![
                    OntologyAttribute {
                        name: "full_name".to_string(),
                        r#type: "text".to_string(),
                        description: "Full name of the person".to_string(),
                    },
                    OntologyAttribute {
                        name: "role".to_string(),
                        r#type: "text".to_string(),
                        description: "Role or occupation".to_string(),
                    },
                ],
                examples: vec!["ordinary citizen".to_string(), "anonymous netizen".to_string()],
            });
        }

        if !has_org {
            fallbacks.push(EntityTypeDef {
                name: "Organization".to_string(),
                description: "Any organization not fitting other specific organization types.".to_string(),
                attributes: vec![
                    OntologyAttribute {
                        name: "org_name".to_string(),
                        r#type: "text".to_string(),
                        description: "Name of the organization".to_string(),
                    },
                    OntologyAttribute {
                        name: "org_type".to_string(),
                        r#type: "text".to_string(),
                        description: "Type of organization".to_string(),
                    },
                ],
                examples: vec!["small business".to_string(), "community group".to_string()],
            });
        }

        if !fallbacks.is_empty() {
            let needed = fallbacks.len();
            let current = ontology.entity_types.len();
            if current + needed > MAX_ENTITY_TYPES {
                let to_remove = current + needed - MAX_ENTITY_TYPES;
                let keep = current.saturating_sub(to_remove);
                ontology.entity_types.truncate(keep);
            }
            ontology.entity_types.extend(fallbacks);
        }

        // Final safeguard
        ontology.entity_types.truncate(MAX_ENTITY_TYPES);
        ontology.edge_types.truncate(MAX_EDGE_TYPES);
    }
}
