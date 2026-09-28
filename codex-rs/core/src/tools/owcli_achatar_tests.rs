//! OwCLI (fork): um provedor sem namespaces recebe as ferramentas do MCP
//! achatadas (`mcp__servidor__ferramenta`), e o registro acha a ferramenta
//! quando o modelo chama por esse nome.

use super::*;
use pretty_assertions::assert_eq;

struct Ferramenta {
    nome: codex_tools::ToolName,
}

impl ToolExecutor<ToolInvocation> for Ferramenta {
    fn tool_name(&self) -> codex_tools::ToolName {
        self.nome.clone()
    }

    fn spec(&self) -> codex_tools::ToolSpec {
        codex_tools::ToolSpec::Function(funcao(&self.nome.name))
    }

    fn handle<'a>(&'a self, _invocation: ToolInvocation) -> codex_tools::ToolExecutorFuture<'a>
    where
        ToolInvocation: 'a,
    {
        Box::pin(async {
            Ok(
                Box::new(crate::tools::context::FunctionToolOutput::from_text(
                    "ok".to_string(),
                    Some(true),
                )) as Box<dyn crate::tools::context::ToolOutput>,
            )
        })
    }
}

impl CoreToolRuntime for Ferramenta {}

fn funcao(nome: &str) -> codex_tools::ResponsesApiTool {
    codex_tools::ResponsesApiTool {
        name: nome.to_string(),
        description: "Ferramenta de teste.".to_string(),
        strict: false,
        defer_loading: None,
        parameters: codex_tools::JsonSchema::default(),
        output_schema: None,
    }
}

fn runtime(nome: codex_tools::ToolName) -> Arc<dyn CoreToolRuntime> {
    Arc::new(Ferramenta { nome }) as Arc<dyn CoreToolRuntime>
}

#[test]
fn o_nome_achatado_junta_namespace_e_ferramenta_com_um_separador_so() {
    use codex_tools::ToolName;
    assert_eq!(
        nome_achatado(&ToolName::namespaced("mcp__eco__", "eco")),
        "mcp__eco__eco"
    );
    assert_eq!(
        nome_achatado(&ToolName::namespaced("mcp__codex_apps__gmail", "send")),
        "mcp__codex_apps__gmail__send"
    );
    assert_eq!(
        nome_achatado(&ToolName::namespaced("functions", "shell")),
        "shell"
    );
    assert_eq!(nome_achatado(&ToolName::plain("shell")), "shell");
}

#[test]
fn o_registro_acha_a_ferramenta_do_mcp_pelo_nome_achatado() {
    use codex_tools::ToolName;
    let do_mcp = runtime(ToolName::namespaced("mcp__eco__", "eco"));
    let comum = runtime(ToolName::plain("shell"));
    let registro = ToolRegistry::from_tools([Arc::clone(&do_mcp), Arc::clone(&comum)]);

    // O modelo chama sem namespace: vira o padrão, e o nome achatado resolve.
    let chamada = ToolName::plain("mcp__eco__eco");
    assert!(
        registro
            .tool(&chamada)
            .is_some_and(|t| Arc::ptr_eq(&t, &do_mcp))
    );
    assert_eq!(
        registro.chave(&chamada),
        Some(ToolName::namespaced("mcp__eco__", "eco"))
    );
    assert_eq!(
        registro.supports_parallel_tool_calls(&chamada).is_some(),
        true
    );
    // Os nomes de sempre continuam como eram.
    assert!(
        registro
            .tool(&ToolName::plain("shell"))
            .is_some_and(|t| Arc::ptr_eq(&t, &comum))
    );
    assert!(registro.tool(&ToolName::plain("mcp__eco__outra")).is_none());
    // Um namespace explícito errado não cai no atalho.
    assert!(
        registro
            .tool(&ToolName::namespaced("mcp__x__", "mcp__eco__eco"))
            .is_none()
    );
}

#[test]
fn uma_funcao_com_o_mesmo_nome_vence_o_atalho() {
    use codex_tools::ToolName;
    let do_mcp = runtime(ToolName::namespaced("mcp__eco__", "eco"));
    let homonima = runtime(ToolName::plain("mcp__eco__eco"));
    let registro = ToolRegistry::from_tools([Arc::clone(&do_mcp), Arc::clone(&homonima)]);
    assert!(
        registro
            .tool(&ToolName::plain("mcp__eco__eco"))
            .is_some_and(|t| Arc::ptr_eq(&t, &homonima))
    );
}

#[test]
fn as_especificacoes_em_namespace_viram_funcoes_de_primeiro_nivel() {
    use codex_tools::ResponsesApiNamespace;
    use codex_tools::ResponsesApiNamespaceTool;
    let specs = vec![
        codex_tools::ToolSpec::Function(funcao("shell")),
        codex_tools::ToolSpec::Namespace(ResponsesApiNamespace {
            name: "mcp__eco__".to_string(),
            description: "Tools in the mcp__eco__ namespace.".to_string(),
            tools: vec![
                ResponsesApiNamespaceTool::Function(funcao("eco")),
                ResponsesApiNamespaceTool::Function(funcao("inverter")),
            ],
        }),
    ];
    let nomes: Vec<String> = crate::tools::spec_plan::achatar_namespaces(specs)
        .iter()
        .map(|s| match s {
            codex_tools::ToolSpec::Function(f) => f.name.clone(),
            outra => panic!("sobrou algo que não é função: {outra:?}"),
        })
        .collect();
    assert_eq!(nomes, ["shell", "mcp__eco__eco", "mcp__eco__inverter"]);
}
