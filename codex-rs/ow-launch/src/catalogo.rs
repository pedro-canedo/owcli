//! O catálogo de modelos no esquema interno do Codex, a partir do que o app
//! manda.
//!
//! O `ModelInfo` tem dezenas de campos obrigatórios e muda a cada versão do
//! upstream. Em vez de escrever o JSON à mão, uma entrada do catálogo embutido
//! serve de molde e só o que importa para um modelo local é trocado — com os
//! tipos reais, então um campo renomeado quebra aqui, na compilação.

use codex_protocol::config_types::ReasoningSummary;
use codex_protocol::openai_models::InputModality;
use codex_protocol::openai_models::ModelInfo;
use codex_protocol::openai_models::ModelVisibility;
use codex_protocol::openai_models::ModelsResponse;
use codex_protocol::openai_models::ReasoningEffort;
use codex_protocol::openai_models::ReasoningEffortPreset;
use codex_protocol::openai_models::WebSearchToolType;

use crate::ModeloDoApp;

/// A entrada do catálogo embutido usada como molde: a primeira sem
/// `tool_mode` (os modelos em code mode dependem de um host que não temos).
fn molde() -> Result<ModelInfo, String> {
    let embutido = codex_models_manager::bundled_models_response().map_err(|e| e.to_string())?;
    embutido
        .models
        .iter()
        .find(|m| m.tool_mode.is_none())
        .or_else(|| embutido.models.first())
        .cloned()
        .ok_or_else(|| "o catálogo embutido do upstream está vazio".to_string())
}

fn esforco(nome: &str) -> Option<ReasoningEffort> {
    Some(match nome {
        "none" => ReasoningEffort::None,
        "minimal" => ReasoningEffort::Minimal,
        "low" => ReasoningEffort::Low,
        "medium" => ReasoningEffort::Medium,
        "high" => ReasoningEffort::High,
        "xhigh" => ReasoningEffort::XHigh,
        "max" => ReasoningEffort::Max,
        _ => return None,
    })
}

pub fn catalogo(modelos: &[ModeloDoApp]) -> Result<ModelsResponse, String> {
    let molde = molde()?;
    let models = modelos
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let mut info = molde.clone();
            info.slug = m.id.clone();
            info.display_name = m.nome.clone();
            info.description = Some("OpenWeights".to_string());
            info.priority = i32::try_from(i).unwrap_or(i32::MAX).saturating_add(1);
            info.visibility = ModelVisibility::List;
            info.supported_in_api = true;

            // O contexto por slot do Router; a compactação sai dele (90%).
            info.context_window = m.janela;
            info.max_context_window = m.janela;
            info.auto_compact_token_limit = None;
            info.comp_hash = None;

            // Só os níveis que o chat template aceita: pedir um nível que o
            // template não conhece derruba o turno inteiro.
            info.supported_reasoning_levels = m
                .esforcos
                .iter()
                .filter_map(|e| esforco(e))
                .map(|effort| ReasoningEffortPreset {
                    effort,
                    description: "Raciocínio do modelo / Model reasoning".to_string(),
                })
                .collect();
            info.default_reasoning_level = info
                .supported_reasoning_levels
                .iter()
                .map(|p| p.effort.clone())
                .find(|e| *e == ReasoningEffort::Medium)
                .or_else(|| {
                    info.supported_reasoning_levels
                        .first()
                        .map(|p| p.effort.clone())
                });
            info.supports_reasoning_summary_parameter = false;
            info.default_reasoning_summary = ReasoningSummary::None;
            info.supports_reasoning_effort_updates = false;
            info.support_verbosity = false;
            info.default_verbosity = None;

            // O shim do llama.cpp só aceita ferramentas `function`: sem
            // apply_patch freeform (a edição vai pelo shell) e sem busca.
            info.apply_patch_tool_type = None;
            info.web_search_tool_type = WebSearchToolType::Text;
            info.supports_search_tool = false;
            info.experimental_supported_tools = Vec::new();
            info.tool_mode = None;
            info.multi_agent_version = None;
            info.multi_agent_reasoning_effort = None;
            info.use_responses_lite = false;
            info.supports_experimental_context = false;

            info.input_modalities = if m.imagem {
                vec![InputModality::Text, InputModality::Image]
            } else {
                vec![InputModality::Text]
            };
            info.supports_image_detail_original = false;

            // Nada de planos, tiers e avisos de conta da OpenAI.
            info.service_tiers = Vec::new();
            info.additional_speed_tiers = Vec::new();
            info.default_service_tier = None;
            info.available_access_programs = None;
            info.availability_nux = None;
            info.upgrade = None;
            info.auto_review_model_override = None;
            info.model_specialty = None;
            info
        })
        .collect();
    Ok(ModelsResponse { models })
}
