# OwCLI — o fork

O OwCLI é o agente de código de terminal do [OpenWeights](https://github.com/pedro-canedo/openweights),
um fork do [OpenAI Codex CLI](https://github.com/openai/codex) (Apache-2.0) que roda com os
modelos do app: o Servidor Local (llama.cpp), o OpenRouter e o 9router. Ele abre dentro do app,
em terminais embutidos, e também no terminal do sistema como `owcli`.

Não é um produto da OpenAI e não é endossado por ela. "Codex" e "OpenAI" são marcas da OpenAI;
o nome do produto é OwCLI e a relação aparece só como "baseado no OpenAI Codex CLI".

## Regras do fork

1. **O núcleo é do upstream.** Tudo o que é nosso mora em crates `codex-rs/ow-*`. Arquivo do
   upstream só é editado onde não há costura, e cada edição entra na tabela abaixo com o
   motivo e o jeito de reaplicar.
2. **O cérebro vem sempre do OpenWeights.** O OwCLI não tem provedor, chave nem login
   próprios: o app escreve `openweights.json` na casa do OwCLI, e o lançador (`ow-launch`)
   monta dali o provedor, o catálogo e o token.
3. **Nada sai da máquina.** Telemetria, feedback, checagem de versão e os recursos que falam
   com serviços da OpenAI ficam desligados pelo lançador (lista em `ow-launch/src/lib.rs`).
4. **Sincronização só por tag estável** (`rust-vX.Y.Z`), com merge — nunca rebase — num PR
   `sync/upstream-<tag>`. O `Cargo.lock` é regenerado, não mesclado à mão.
5. Os nomes internos continuam `codex-*`: renomear transformaria cada sincronização em
   conflito. Muda o que a pessoa vê.

Base atual: `rust-v0.157.1` (2026-09-26).

## Arquivos do upstream editados

| Arquivo | O que muda | Por quê | Como reaplicar |
|---|---|---|---|
| `codex-rs/Cargo.toml` | `ow-launch` em `members` e em `[workspace.dependencies]` | o crate nosso entra no workspace | duas linhas, marcadas com `OwCLI (fork)` |
| `codex-rs/cli/Cargo.toml` | dependência `ow-launch` | o `main` chama o lançador | uma linha, marcada |
| `codex-rs/cli/src/main.rs` | `ow_launch::preparar()` na primeira linha do `main`; `MultitoolCli::parse_from(ow_launch::argumentos())` no lugar de `parse()` | a linha de comando ganha os `-c` do OwCLI antes do clap; a casa é definida antes de qualquer thread | duas linhas |
| `codex-rs/tui/src/chatwidget.rs` | `marca_owcli()` e `placeholder_do_compositor()` | a marca troca só no produto | duas funções, marcadas |
| `codex-rs/tui/src/chatwidget/constructor.rs` | o compositor usa `placeholder_do_compositor()` | marca | uma linha |
| `codex-rs/tui/src/startup_draft.rs` | idem, no rascunho inicial | marca | uma linha |
| `codex-rs/tui/src/history_cell/session.rs` | cabeçalho "OwCLI (vX)" quando `marca_owcli()` | marca | duas expressões |
| `codex-rs/tui/src/status/card.rs` | "OwCLI" no `/status` quando `marca_owcli()` | marca | uma expressão |
| `codex-rs/exec/src/event_processor_with_human_output.rs` | cabeçalho "OwCLI v…" do `exec` com `OWCLI` definido | marca | uma expressão |

**A marca só troca no produto.** O lançador define `OWCLI=1` no ambiente; os pontos acima
escolhem o texto por ela. Os testes do upstream não definem a variável, então veem o texto
original: nenhum teste e nenhum snapshot do upstream muda (são 289 snapshots com "Codex" e
dezenas de testes com a marca escrita — trocar todos transformaria cada versão em horas de
conflito). O resto dos textos com "Codex" fica para uma camada própria.

**O binário continua `codex` no cargo.** 39 arquivos de teste do upstream o procuram por esse
nome. O pacote do runtime entrega o mesmo executável como `owcli` (`owcli.exe` no Windows), e o
modo vem do nome: `owcli` é sempre o produto; `codex` só vira OwCLI com `OWCLI_HOME` definido
(assim o build do cargo e os testes do upstream rodam como o upstream).

**Testes do upstream que falham fora da CI deles** (conferido com o código original, não é o
fork): a biblioteca da TUI precisa de `RUST_MIN_STACK=16777216` (um teste estoura a pilha
padrão e aborta o binário inteiro), e alguns testes de worktree, git e largura de cabeçalho
dependem do ambiente.

A licença Apache-2.0 pede aviso nos arquivos modificados: esta tabela é esse aviso, e cada
trecho editado leva um comentário `OwCLI (fork)`.

## O contrato com o app (`openweights.json`, versão 1)

O app escreve o arquivo na casa do OwCLI (`OWCLI_HOME`, padrão `~/.owcli`) de forma atômica e
com permissão só do dono:

```json
{
  "versao": 1,
  "baseUrl": "http://127.0.0.1:<porta>/owcli/v1",
  "token": "<token do gateway do app>",
  "modelos": [
    { "id": "local:Qwen3-Coder-30B", "nome": "Qwen3 Coder 30B", "janela": 65536, "esforcos": [] }
  ],
  "modeloPadrao": "local:Qwen3-Coder-30B"
}
```

- `janela` é o contexto **por slot** do Router; a compactação do Codex dispara em 90% dele.
- `esforcos` são só os níveis que o chat template aceita: pedir um nível desconhecido derruba o
  turno inteiro. Vazio = modelo sem raciocínio.
- O token nunca passa por argv nem por variável de ambiente: o Codex o pede ao próprio binário
  (`owcli --ow-token`, via `auth.command`) e renova a cada minuto.
- O modelo padrão só entra quando a `config.toml` da pessoa não escolhe um.

Sem o arquivo, o que conversa com modelo sai com uma mensagem pedindo para abrir o app;
`--help`, `--version`, `app-server`, `sandbox` e afins rodam normalmente.

## O que foi medido (spike de 2026-09-28)

Codex 0.157.1 **sem modificação** contra o llama-server b10441 do OpenWeights
(`/v1/responses`, Vulkan, RTX 3090):

- 4 de 5 tarefas (ler, editar, rodar teste e consertar, vários turnos com raciocínio) com
  **zero reescrita de requisição**, com Qwen3-Coder-30B (Q2_K) e Qwen3-8B.
- Com os resumos de raciocínio desligados no catálogo, o raciocínio volta com `summary: []` e
  o shim do llama.cpp aceita (o bug llama.cpp#29159 é com `null`).
- **MCP não funciona** com modelo local: o Codex manda as ferramentas MCP como
  `type: "namespace"`, e o shim do llama.cpp só aceita `function` (llama.cpp#24295). Fica para
  uma costura própria (achatar por provedor).
- O `apply_patch` do upstream só existe como ferramenta `freeform`; com modelo local a edição
  vai pelo shell (`apply_patch` como comando, `sed`, reescrita), e funciona.
- Rede, com `strace`: sem os desligamentos do lançador, o Codex fala com chatgpt.com e com o
  GitHub mesmo com a telemetria desligada. Com eles, **nenhuma conexão externa**.
- O `codex-linux-sandbox` (bwrap) bloqueia escrita fora do workspace no Fedora Silverblue 44.
- Build de release no Linux: ~14 min frio em 16 núcleos; 277 MB sem símbolos, 105 MB em
  tar.gz.

Achado de uso: o `exec` sem config roda com sandbox somente leitura e aprovação "nunca" — o
modelo pede para escrever e o Codex recusa. O modo de sandbox e de aprovação é escolha da
pessoa (o app oferece ao abrir a sessão); o lançador não força.
