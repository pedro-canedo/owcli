use super::*;
use pretty_assertions::assert_eq;

fn conexao() -> Conexao {
    Conexao {
        versao: 1,
        base_url: "http://127.0.0.1:11740/owcli/v1".into(),
        token: Some("segredo".into()),
        modelos: vec![
            ModeloDoApp {
                id: "local:qwen3-coder".into(),
                nome: "Qwen3 Coder 30B".into(),
                janela: Some(65_536),
                esforcos: vec![],
                imagem: false,
            },
            ModeloDoApp {
                id: "local:qwen3-8b".into(),
                nome: "Qwen3 8B".into(),
                janela: Some(32_768),
                esforcos: vec!["low".into(), "medium".into(), "ultra-desconhecido".into()],
                imagem: true,
            },
        ],
        modelo_padrao: None,
    }
}

fn texto(v: &[OsString]) -> Vec<String> {
    v.iter().map(|a| a.to_string_lossy().into_owned()).collect()
}

fn overrides(v: &[String]) -> Vec<String> {
    v.windows(2)
        .filter(|w| w[0] == "-c")
        .map(|w| w[1].clone())
        .collect()
}

#[test]
fn os_overrides_entram_depois_do_arg0_e_a_linha_da_pessoa_segue_intacta() {
    let casa = tempfile::tempdir().unwrap();
    let argv = ["owcli", "exec", "-m", "outro", "faça algo"]
        .map(OsString::from)
        .to_vec();
    let saida =
        texto(&montar(argv, casa.path(), Some(&conexao()), Path::new("/bin/owcli")).unwrap());
    assert_eq!(saida[0], "owcli");
    assert_eq!(saida[1], "-c");
    assert_eq!(
        &saida[saida.len() - 4..],
        ["exec", "-m", "outro", "faça algo"]
    );
    let o = overrides(&saida);
    for esperado in [
        "analytics.enabled=false",
        "features.daemon_auto_start=false",
        "features.apps=false",
        "model_provider=\"openweights\"",
        "model_providers.openweights.base_url=\"http://127.0.0.1:11740/owcli/v1\"",
        "model_providers.openweights.wire_api=\"responses\"",
        "model_providers.openweights.auth.args=[\"--ow-token\"]",
        "model=\"local:qwen3-coder\"",
        "tui.notification_method=\"osc9\"",
    ] {
        assert!(
            o.iter().any(|x| x == esperado),
            "faltou {esperado} em {o:?}"
        );
    }
    // O token nunca vai na linha de comando.
    assert!(!saida.iter().any(|a| a.contains("segredo")));
}

#[test]
fn modelo_da_config_da_pessoa_vence_o_padrao_do_app() {
    let casa = tempfile::tempdir().unwrap();
    std::fs::write(
        casa.path().join("config.toml"),
        "model = \"local:qwen3-8b\"\n",
    )
    .unwrap();
    let saida = texto(
        &montar(
            vec!["owcli".into()],
            casa.path(),
            Some(&conexao()),
            Path::new("owcli"),
        )
        .unwrap(),
    );
    assert!(!overrides(&saida).iter().any(|o| o.starts_with("model=")));
}

#[test]
fn sem_o_app_so_roda_o_que_nao_precisa_de_modelo() {
    let casa = tempfile::tempdir().unwrap();
    let erro = montar(vec!["owcli".into()], casa.path(), None, Path::new("owcli")).unwrap_err();
    assert!(erro.contains("Abra o OpenWeights"));
    for ok in [
        vec!["owcli", "--help"],
        vec!["owcli", "app-server", "--stdio"],
        vec!["owcli", "sandbox", "--", "ls"],
    ] {
        let argv = ok.into_iter().map(OsString::from).collect();
        assert!(montar(argv, casa.path(), None, Path::new("owcli")).is_ok());
    }
}

#[test]
fn caminho_do_windows_vira_string_toml_valida() {
    let s = toml_str(r#"C:\Users\Pedro "P"\owcli.exe"#);
    let lido: toml::Table = format!("x = {s}").parse().unwrap();
    assert_eq!(
        lido["x"].as_str().unwrap(),
        r#"C:\Users\Pedro "P"\owcli.exe"#
    );
}

#[test]
fn o_catalogo_tem_os_modelos_do_app_e_volta_pelo_serde_do_codex() {
    let casa = tempfile::tempdir().unwrap();
    let caminho = escrever_catalogo(casa.path(), &conexao().modelos).unwrap();
    let lido: ModelsResponse =
        serde_json::from_str(&std::fs::read_to_string(caminho).unwrap()).unwrap();
    let slugs: Vec<&str> = lido.models.iter().map(|m| m.slug.as_str()).collect();
    assert_eq!(slugs, ["local:qwen3-coder", "local:qwen3-8b"]);

    let coder = &lido.models[0];
    assert_eq!(coder.context_window, Some(65_536));
    assert!(coder.supported_reasoning_levels.is_empty());
    assert_eq!(coder.default_reasoning_level, None);
    assert_eq!(coder.apply_patch_tool_type, None);
    assert!(!coder.supports_search_tool);
    assert_eq!(coder.input_modalities, vec![InputModality::Text]);

    let oito = &lido.models[1];
    let niveis: Vec<ReasoningEffort> = oito
        .supported_reasoning_levels
        .iter()
        .map(|p| p.effort.clone())
        .collect();
    // O nível que o Codex não conhece é descartado, não inventado.
    assert_eq!(niveis, [ReasoningEffort::Low, ReasoningEffort::Medium]);
    assert_eq!(oito.default_reasoning_level, Some(ReasoningEffort::Medium));
    assert_eq!(
        oito.input_modalities,
        vec![InputModality::Text, InputModality::Image]
    );
}

#[test]
fn o_nome_do_binario_decide_o_modo() {
    let de = |s: &str| OsString::from(s);
    assert_eq!(modo(Some(&de("/opt/ow/owcli")), false), Modo::OwCli);
    #[cfg(windows)]
    assert_eq!(modo(Some(&de(r"C:\ow\owcli.exe")), false), Modo::OwCli);
    // O build do cargo (e os testes do upstream) só vira OwCLI com OWCLI_HOME.
    assert_eq!(
        modo(Some(&de("/target/release/codex")), false),
        Modo::Upstream
    );
    assert_eq!(modo(Some(&de("/target/release/codex")), true), Modo::OwCli);
    assert_eq!(
        modo(Some(&de("/tmp/x/codex-linux-sandbox")), false),
        Modo::Auxiliar
    );
    assert_eq!(modo(Some(&de("apply_patch")), true), Modo::Auxiliar);
}

use codex_protocol::openai_models::InputModality;
use codex_protocol::openai_models::ModelsResponse;
use codex_protocol::openai_models::ReasoningEffort;
