# ADR-0028 — Tarefas Cargo do plugin Rust

Status: implementada em 2026-10-02.

## Problema e decisão

O plugin Rust analisava, navegava e editava código, mas não compilava nem testava.
Tasks existiam somente para runtimes externos, que descrevem um `ProcessRequest`
em JSON. Cargo precisa de recursos que um plugin externo nunca deve poder pedir:
a toolchain revisada, as fontes do registry e um diretório de build gravável.

O núcleo passa a definir as tarefas `cargo check`, `cargo build`, `cargo test` e,
quando há binário (`src/main.rs` ou `[[bin]]`), `cargo run`. Elas só aparecem com
`Cargo.toml` regular na raiz do workspace e com o plugin Rust embutido instalado
na versão atual, ativo, com grants exatos e com a capability `tasks`. O manifesto
foi para 0.1.2; instalações anteriores precisam ser revisadas novamente.

Cada execução usa o mesmo broker, revisão de uso único, hash da ação, streaming,
timeout, cancelamento do grupo de processos e auditoria das outras Tasks. Ao
desativar ou remover o plugin, revisões pendentes e execuções em andamento são
invalidadas pelo mesmo caminho dos plugins externos.

## Sandbox

O `ProcessBroker` aceita uma `SandboxExtension` construída apenas por código Rust
do núcleo. Ela não é desserializável; o catálogo de um plugin continua limitado a
`ProcessRequest`. A extensão:

- só vale no Bubblewrap e é recusada no modo `escalated`;
- monta destinos somente sob `/toolchain` ou subdiretórios de `/tmp`, nunca o
  próprio `/tmp`, `/workspace`, `/usr` ou caminhos com `..`;
- canonicaliza a origem e recusa o workspace, seus descendentes e seus ancestrais,
  para que o modo de acesso declarado no request continue valendo;
- acrescenta variáveis com nomes em maiúsculas, recusando `HOME`, `TMPDIR` e
  variáveis do loader;
- entra no hash da aprovação e aparece na revisão. Qualquer mount gravável eleva
  o risco para `approval_required`.

As tarefas Cargo usam workspace somente leitura, sem rede, `--offline --locked` e
um ambiente limpo. A toolchain e o registry aprovados na
[ADR-0023](ADR-0023-rust-toolchains-and-local-dependencies.md) são montados como
leitura; sem revisão, vale a toolchain do sistema. `CARGO_HOME` é temporário,
wrappers do rustc são esvaziados e `RUSTDOC` só é definido quando a própria
toolchain o fornece. O build vai para `cache/cargo-target/<sha256 do workspace>`,
diretório 0700 do Lyrnova montado como `/tmp/target`, preservando builds
incrementais sem escrever `target/` no projeto.

## Consequências

`cargo build`, `test` e `run` executam build scripts, macros procedurais, testes e
o binário do projeto. A revisão diz isso explicitamente; a fronteira efetiva é o
sandbox. A jornada nativa usa um `build.rs` hostil: ele roda, mas não consegue
escrever no workspace (`Read-only file system`).

Como o workspace é somente leitura, o projeto precisa de `Cargo.lock` e de todas
as dependências disponíveis offline; a lista avisa quando falta o lockfile.
Programas que gravam arquivos no projeto, dependências Git, fontes fora do
registry aprovado e build scripts que esperam rede falham. Os limites de processo
da [ADR-0016](ADR-0016-process-broker.md) também valem: 2 GiB de memória virtual
e 64 MiB por arquivo podem ser insuficientes para projetos grandes, e devem ser
revistos junto da migração para cgroup v2.

Um runtime externo que falha ao listar Tasks não esconde mais as demais: a lista
inclui as falhas por plugin e mantém os outros providers utilizáveis.

Pendências do plugin Rust (#39): erros do Cargo navegáveis, seleção de pacote e
teste individual, templates de projeto, painel de testes e depuração via DAP.
