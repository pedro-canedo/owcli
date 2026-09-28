//! OwCLI: o lançador que faz o Codex virar o agente do OpenWeights.
//!
//! É o único código nosso que o `main` do upstream chama, antes de parsear a
//! linha de comando (duas linhas em `cli/src/main.rs`, registradas no
//! FORK.md). Tudo o que muda no comportamento sai daqui:
//!
//! - **Casa própria.** `OWCLI_HOME` (padrão `~/.owcli`) vira o `CODEX_HOME`:
//!   sessões, configurações e histórico nunca se misturam com um Codex
//!   instalado na mesma máquina.
//! - **O cérebro vem do app.** O OpenWeights escreve `openweights.json` na
//!   casa (endereço, token, modelos). Daqui saem o provedor `openweights`, o
//!   catálogo de modelos no esquema interno do Codex — montado com os tipos
//!   reais, então uma mudança de esquema no upstream quebra na compilação do
//!   merge, não na máquina de ninguém — e o token, que o Codex pede a este
//!   mesmo binário (`owcli --ow-token`) a cada renovação: ele nunca passa
//!   por argv nem por variável de ambiente.
//! - **Nada sai da máquina.** Telemetria, feedback, checagem de versão e os
//!   recursos que falam com serviços da OpenAI entram desligados como `-c` da
//!   raiz. Medido com strace: sem os `--disable` de apps, plugins e cia., o
//!   Codex ainda fala com chatgpt.com e com o GitHub. Quem quiser religa um
//!   por um — flag de subcomando vence a da raiz.
//! - **Sem daemon.** O daemon destacado sobreviveria ao app e rodaria
//!   comandos no ambiente de quem o subiu.

mod catalogo;

use std::ffi::OsString;
use std::net::TcpStream;
use std::net::ToSocketAddrs;
use std::path::Path;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Duration;

use serde::Deserialize;

pub use catalogo::catalogo;

/// Onde mora a casa do OwCLI.
pub const HOME_VAR: &str = "OWCLI_HOME";
/// O arquivo que o OpenWeights escreve na casa.
pub const ARQUIVO_DO_APP: &str = "openweights.json";
/// O id do provedor que o lançador registra.
pub const PROVEDOR: &str = "openweights";
/// Pedido do token (o `auth.command` do provedor aponta para cá).
const FLAG_TOKEN: &str = "--ow-token";
/// Quanto esperar o gateway local aceitar a conexão. É loopback: quando o
/// app está aberto, responde em microssegundos.
const ESPERA_DO_GATEWAY: Duration = Duration::from_millis(800);

/// O que o app escreve em `openweights.json` (contrato v1).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Conexao {
    pub versao: u32,
    /// Base OpenAI-compatível, com `/v1`.
    pub base_url: String,
    #[serde(default)]
    pub token: Option<String>,
    #[serde(default)]
    pub modelos: Vec<ModeloDoApp>,
    #[serde(default)]
    pub modelo_padrao: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModeloDoApp {
    /// Slug que vai no request (`local:Qwen3-Coder-30B`, `openrouter:…`).
    pub id: String,
    pub nome: String,
    /// Contexto por slot, em tokens.
    #[serde(default)]
    pub janela: Option<i64>,
    /// Níveis de esforço que o chat template aceita (vazio: sem raciocínio).
    #[serde(default)]
    pub esforcos: Vec<String>,
    #[serde(default)]
    pub imagem: bool,
}

/// Recursos que falam com serviços da OpenAI (ou sobem o daemon).
const DESLIGADOS: &[&str] = &[
    "apps",
    "plugins",
    "remote_plugin",
    "plugin_sharing",
    "recommended_plugins",
    "tool_suggest",
    "in_app_updates",
    "in_app_chat",
    "in_app_dictation",
    "daemon_auto_start",
    "multi_agent",
    "image_generation",
    "browser_use",
    "browser_use_external",
    "computer_use",
    "realtime_conversation",
    "skill_mcp_dependency_install",
    "skill_search",
    "fast_mode",
];

/// `-c` fixos: nada de telemetria, e o "precisa de você" pelo OSC 9, que o
/// app lê para marcar a sessão e avisar com a janela sem foco.
fn fixos() -> Vec<(&'static str, String)> {
    let mut v = vec![
        ("analytics.enabled", "false".to_string()),
        ("feedback.enabled", "false".to_string()),
        ("check_for_update_on_startup", "false".to_string()),
        ("otel.exporter", toml_str("none")),
        ("otel.trace_exporter", toml_str("none")),
        ("otel.metrics_exporter", toml_str("none")),
        ("tui.notification_method", toml_str("osc9")),
        ("tui.notification_condition", toml_str("always")),
        // As dicas falam de recursos do Codex e do ChatGPT que o OwCLI não tem.
        ("tui.show_tooltips", "false".to_string()),
    ];
    if cfg!(windows) {
        // `elevated` cria usuários locais e exige administrador.
        v.push(("windows.sandbox", toml_str("unelevated")));
    }
    v
}

/// Subcomandos que não conversam com modelo: rodam sem o app.
const SEM_MODELO: &[&str] = &[
    "agents",
    "app-server",
    "archive",
    "completion",
    "debug",
    "delete",
    "doctor",
    "exec-server",
    "features",
    "help",
    "login",
    "logout",
    "mcp",
    "migrate-rollouts",
    "plugin",
    "sandbox",
    "unarchive",
];

static ARGUMENTOS: OnceLock<Vec<OsString>> = OnceLock::new();

/// Chamado na primeira linha do `main`, antes de qualquer thread.
///
/// Define a casa, responde ao `--ow-token` e monta a linha de comando que o
/// `main` do upstream vai parsear (leia com [`argumentos`]).
///
/// O modo vem do nome do binário: o pacote entrega `owcli`, e aí é sempre o
/// OwCLI. O cargo gera `codex` — é esse que os testes do upstream rodam, com
/// um `CODEX_HOME` próprio —, e ele só vira OwCLI com `OWCLI_HOME` definido;
/// sem isso, comporta-se exatamente como o upstream.
pub fn preparar() {
    let argv: Vec<OsString> = std::env::args_os().collect();
    if modo(argv.first(), std::env::var_os(HOME_VAR).is_some()) != Modo::OwCli {
        let _ = ARGUMENTOS.set(argv);
        return;
    }
    let casa = casa();
    let _ = std::fs::create_dir_all(&casa);
    // SAFETY: primeira linha do `main`, com o processo ainda de uma thread
    // só — ninguém lê o ambiente em paralelo.
    unsafe {
        std::env::set_var("CODEX_HOME", &casa);
        // A marca do produto nas telas (cabeçalho, placeholder, `exec`).
        std::env::set_var("OWCLI", "1");
    }

    let conexao = ler_conexao(&casa);
    if argv.get(1).is_some_and(|a| a == FLAG_TOKEN) {
        let token = conexao.and_then(|c| c.token).unwrap_or_default();
        println!("{token}");
        std::process::exit(0);
    }

    // O arquivo fica na casa depois de o app fechar: sem esta checagem, a TUI
    // abriria e cada pedido falharia só depois das novas tentativas.
    if let Some(c) = conexao.as_ref()
        && precisa_de_modelo(&argv)
        && !gateway_responde(&c.base_url, ESPERA_DO_GATEWAY)
    {
        eprintln!("{}", app_fechado(&c.base_url));
        std::process::exit(2);
    }

    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("owcli"));
    match montar(argv, &casa, conexao.as_ref(), &exe) {
        Ok(montados) => {
            let _ = ARGUMENTOS.set(montados);
        }
        Err(mensagem) => {
            eprintln!("{mensagem}");
            std::process::exit(2);
        }
    }
}

/// A linha de comando montada por [`preparar`] (a original, se não foi chamado).
pub fn argumentos() -> Vec<OsString> {
    ARGUMENTOS
        .get()
        .cloned()
        .unwrap_or_else(|| std::env::args_os().collect())
}

/// `OWCLI_HOME`, ou `~/.owcli`.
pub fn casa() -> PathBuf {
    if let Some(v) = std::env::var_os(HOME_VAR).filter(|v| !v.is_empty()) {
        return PathBuf::from(v);
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".owcli")
}

#[derive(Debug, PartialEq, Eq)]
enum Modo {
    /// O produto: casa, provedor e catálogo do OpenWeights.
    OwCli,
    /// O binário do upstream sem mudança nenhuma (build do cargo, testes).
    Upstream,
    /// Chamado como auxiliar (sandbox do Linux, apply_patch): o arg0 decide e
    /// a linha de comando não é nossa.
    Auxiliar,
}

fn modo(argv0: Option<&OsString>, owcli_home_definido: bool) -> Modo {
    let nome = argv0
        .and_then(|a| Path::new(a).file_stem())
        .map(|n| n.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    match nome.as_str() {
        "owcli" => Modo::OwCli,
        "codex" if owcli_home_definido => Modo::OwCli,
        "codex" => Modo::Upstream,
        _ => Modo::Auxiliar,
    }
}

fn ler_conexao(casa: &Path) -> Option<Conexao> {
    let texto = std::fs::read_to_string(casa.join(ARQUIVO_DO_APP)).ok()?;
    serde_json::from_str(&texto).ok()
}

/// A linha de comando com os `-c` do OwCLI logo depois do arg0.
pub fn montar(
    argv: Vec<OsString>,
    casa: &Path,
    conexao: Option<&Conexao>,
    exe: &Path,
) -> Result<Vec<OsString>, String> {
    let mut overrides: Vec<String> = fixos()
        .into_iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect();
    overrides.extend(DESLIGADOS.iter().map(|f| format!("features.{f}=false")));

    match conexao {
        Some(c) => overrides.extend(provedor(c, casa, exe)?),
        None if precisa_de_modelo(&argv) => return Err(sem_app(casa)),
        None => {}
    }

    let mut saida = Vec::with_capacity(argv.len() + overrides.len() * 2);
    let mut resto = argv.into_iter();
    saida.push(resto.next().unwrap_or_else(|| OsString::from("owcli")));
    for o in overrides {
        saida.push(OsString::from("-c"));
        saida.push(OsString::from(o));
    }
    saida.extend(resto);
    Ok(saida)
}

/// O provedor, o catálogo, o modelo padrão e o token.
fn provedor(c: &Conexao, casa: &Path, exe: &Path) -> Result<Vec<String>, String> {
    let p = format!("model_providers.{PROVEDOR}");
    let mut v = vec![
        format!("model_provider={}", toml_str(PROVEDOR)),
        format!("{p}.name={}", toml_str("OpenWeights")),
        format!("{p}.base_url={}", toml_str(&c.base_url)),
        format!("{p}.wire_api={}", toml_str("responses")),
        // Sem resumo de raciocínio: o shim do llama.cpp recusa o item de
        // raciocínio com `summary: null` que o Codex mandaria sem ele.
        format!("model_reasoning_summary={}", toml_str("none")),
    ];
    if c.token.as_deref().is_some_and(|t| !t.is_empty()) {
        v.push(format!(
            "{p}.auth.command={}",
            toml_str(&exe.to_string_lossy())
        ));
        v.push(format!("{p}.auth.args=[{}]", toml_str(FLAG_TOKEN)));
        v.push(format!("{p}.auth.refresh_interval_ms=60000"));
    }
    if !c.modelos.is_empty() {
        let caminho = escrever_catalogo(casa, &c.modelos)?;
        v.push(format!(
            "model_catalog_json={}",
            toml_str(&caminho.to_string_lossy())
        ));
    }
    // O modelo é escolha da pessoa: só entra o padrão do app quando a config
    // dela não diz nenhum.
    if !config_tem_modelo(casa)
        && let Some(padrao) = c
            .modelo_padrao
            .clone()
            .or_else(|| c.modelos.first().map(|m| m.id.clone()))
    {
        v.push(format!("model={}", toml_str(&padrao)));
    }
    Ok(v)
}

fn escrever_catalogo(casa: &Path, modelos: &[ModeloDoApp]) -> Result<PathBuf, String> {
    let dir = casa.join(".ow");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let caminho = dir.join("catalogo.json");
    let json = serde_json::to_string_pretty(&catalogo(modelos)?).map_err(|e| e.to_string())?;
    // Escrita atômica: duas sessões abrindo juntas não leem meio arquivo.
    let temp = dir.join(format!("catalogo.{}.tmp", std::process::id()));
    std::fs::write(&temp, json).map_err(|e| e.to_string())?;
    std::fs::rename(&temp, &caminho).map_err(|e| e.to_string())?;
    Ok(caminho)
}

fn config_tem_modelo(casa: &Path) -> bool {
    std::fs::read_to_string(casa.join("config.toml"))
        .ok()
        .and_then(|t| t.parse::<toml::Table>().ok())
        .is_some_and(|t| t.contains_key("model"))
}

fn precisa_de_modelo(argv: &[OsString]) -> bool {
    let args: Vec<String> = argv
        .iter()
        .skip(1)
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    if args
        .iter()
        .any(|a| matches!(a.as_str(), "-h" | "--help" | "-V" | "--version"))
    {
        return false;
    }
    !args.iter().any(|a| SEM_MODELO.contains(&a.as_str()))
}

fn sem_app(casa: &Path) -> String {
    let arquivo = casa.join(ARQUIVO_DO_APP);
    format!(
        "O OwCLI usa os modelos do OpenWeights, e a ligação com o app não está em {}.\n\
         Abra o OpenWeights e ligue o OwCLI na tela do OwCLI.\n\
         OwCLI runs on OpenWeights models, and the link to the app is missing ({}).\n\
         Open OpenWeights and turn OwCLI on from the OwCLI screen.",
        arquivo.display(),
        arquivo.display()
    )
}

/// O gateway do app aceita conexão? Só abre e fecha o TCP; quem responde
/// pelo resto (token, fonte fora do ar) é o próprio gateway, com mensagem.
pub fn gateway_responde(base_url: &str, espera: Duration) -> bool {
    let Some(endereco) = host_e_porta(base_url) else {
        return false;
    };
    let Ok(destinos) = endereco.to_socket_addrs() else {
        return false;
    };
    destinos
        .into_iter()
        .any(|d| TcpStream::connect_timeout(&d, espera).is_ok())
}

/// `http://127.0.0.1:11740/owcli/v1` → `127.0.0.1:11740`.
fn host_e_porta(base_url: &str) -> Option<String> {
    let (esquema, resto) = base_url.split_once("://")?;
    let autoridade = resto.split(['/', '?', '#']).next()?;
    if autoridade.is_empty() {
        return None;
    }
    // Sem porta explícita, a do esquema (IPv6 vem entre colchetes).
    let tem_porta = autoridade
        .rsplit_once(':')
        .is_some_and(|(_, p)| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()));
    Some(if tem_porta {
        autoridade.to_string()
    } else if esquema.eq_ignore_ascii_case("https") {
        format!("{autoridade}:443")
    } else {
        format!("{autoridade}:80")
    })
}

fn app_fechado(base_url: &str) -> String {
    format!(
        "O OpenWeights não está respondendo em {base_url}.\n\
         Abra o app: o OwCLI pensa com os modelos dele.\n\
         OpenWeights is not answering at {base_url}.\n\
         Open the app: OwCLI runs on its models."
    )
}

/// Uma string TOML básica: aspas e barras escapadas (caminhos do Windows).
pub fn toml_str(s: &str) -> String {
    let mut saida = String::with_capacity(s.len() + 2);
    saida.push('"');
    for c in s.chars() {
        match c {
            '\\' => saida.push_str("\\\\"),
            '"' => saida.push_str("\\\""),
            '\n' => saida.push_str("\\n"),
            '\t' => saida.push_str("\\t"),
            c if c.is_control() => saida.push_str(&format!("\\u{:04X}", c as u32)),
            c => saida.push(c),
        }
    }
    saida.push('"');
    saida
}

#[cfg(test)]
mod tests;
