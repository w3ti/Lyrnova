# ADR-0021 — Diagnósticos Rust por LSP

- Data: 2026-09-25
- Estado: implementado localmente; sem publicação

## Decisão e escopo

O plugin Rust embutido 0.1.1 declara `lsp`, `diagnostics`, `workspace_read` e
`process_spawn`. A adição de execução exige revisão explícita: perfis novos só
recebem automaticamente a concessão de leitura, e a migração revoga a ativação
anterior quando as permissões/versão divergem. A interface permite revisar e ativar
Rust em Configurações ou pelo painel Problemas. Coloração e snippets existentes
continuam locais ao Monaco.

O núcleo possui um adaptador restrito de diagnósticos. Não há IPC genérico para
encaminhar métodos LSP, executar comandos ou aplicar alterações propostas pelo
servidor. Cada start/sync/status revalida identidade, runtime embutido, capabilities,
versão instalada, ativação e grants exatos, sob o mutex de lifecycle de plugins.
Troca de projeto, desativação/remoção do plugin e fechamento da janela encerram a
sessão; uma resposta atrasada não adquire autoridade no workspace novo.

O frontend sincroniza os arquivos Rust abertos usando `didOpen`, mudanças completas
em `didChange` e `didClose`. Cada sessão possui identificador aleatório, e versões
crescem inclusive ao fechar/reabrir um arquivo. Diagnósticos precisam corresponder
à URI de um documento aberto e à versão atual. Publicações sem versão são ignoradas
nesta etapa. A interface remove marcadores imediatamente na edição e usa somente
resultados da sessão/versão corrente. Cursor e ranges usam UTF-16, incluindo pares
surrogados. Mensagens são texto literal no Monaco e no painel Problemas, com
navegação à linha/coluna pelo clique.

## Processo e transporte

No Linux o servidor standalone ELF `rust-analyzer` é localizado em entradas
absolutas do PATH do IDE, fora do workspace. Shims do rustup são recusados: esta
etapa não resolve nem baixa toolchains escolhidas pelo projeto. Não há download
automático de executáveis na aplicação. O teste usa a release oficial
[2026-09-21](https://github.com/rust-lang/rust-analyzer/releases/tag/2026-09-21),
`0.3.3057-standalone`, com SHA-256 verificado antes da extração.

Bubblewrap inicia o servidor com ambiente limpo, todos os namespaces isolados,
capabilities removidas, rede isolada e projeto montado somente para leitura em
`/workspace`. HOME, Cargo home e target são temporários; não se montam caches ou
credenciais do usuário. Bibliotecas/ferramentas do sistema e o servidor têm mounts
read-only. O processo recebe limites de descritores (256), arquivo (64 MiB),
memória virtual (4 GiB), core desativado e `no_new_privs`. `--die-with-parent` e o
namespace PID integram o encerramento dos filhos ao lifecycle.

Stdin/stdout usam framing LSP `Content-Length` limitado, com pipes não bloqueantes
em um worker dedicado; a UI não espera leitura de pipe na thread principal.
Inicialização tem prazo de 15 s; transporte sem progresso, 10 s. Shutdown recebe
até 200 ms antes do término forçado e recolhimento do processo. EOF, timeout,
resposta incompatível ou falha do processo removem os diagnósticos e expõem erro
recuperável pelo botão de reinício. Stderr não é enviado à UI nem registrado.

Pedidos `workspace/configuration` recebem apenas opções fixas do adaptador;
`workspace/applyEdit` é recusado e demais pedidos sem consumidor tipado recebem
`MethodNotFound`. Registro dinâmico e execução de comandos não são concedidos.
Somente UTF-16 e sincronização de texto completa/incremental são aceitos na
negociação; o cliente envia alterações completas permitidas pelo protocolo.

## Análise inicial e limites

As opções do servidor desativam check-on-save, build scripts, proc macros,
carregamento de sysroot, busca de dependências e recarga automática de Cargo.
Metadados locais de Cargo e fontes abertas permitem os diagnósticos iniciais.
Não se trata de `cargo check`, nem de uma integração completa de toolchain: tipos
de dependências externas, biblioteca padrão, cfg gerado por build scripts e
expansões de proc macros podem ficar incompletos. A interface informa esse escopo.

Configuração de workspace pode prevalecer sobre opções LSP no rust-analyzer.
Antes do lançamento, arquivos `rust-analyzer.toml` existentes nas pastas de código
são sobrepostos com conteúdo vazio dentro do sandbox, sem modificar o projeto.
A descoberta não segue symlinks, ignora `.git`, `target` e `node_modules`, aceita
até 50 mil entradas/256 arquivos de configuração e falha explicitamente nos
limites. Arquivos/configurações criados ou substituídos por outro processo durante
a sessão não constituem um snapshot imutável; reiniciar refaz a descoberta. A
fronteira de autoridade é o sandbox de leitura/rede, não a configuração do servidor.
Comandos e wrappers que ferramentas do projeto descubram permanecem dentro dele.

Até 32 documentos, 512 KiB por documento e 8 MiB de texto total são sincronizados.
Excedentes ficam fora da análise com indicação visível. Frames têm até 4 MiB,
headers até 8 KiB e fila de saída até 16 MiB; sincronização pendente é coalescida
pelo estado corrente. São exibidos até 200 diagnósticos por arquivo e mil no total,
com mensagens de até 4 KiB e indicação de truncamento. Paths, revisões, ranges,
severidades e conteúdo são validados antes de chegar à interface.

Definição, referências, hover, completion LSP, rename, formatação, DAP, download
de toolchains, caches externos e outras plataformas continuam pendentes. Estado
LSP não é persistido: um novo processo autorizado analisa a sessão recuperada.

## Evidência e referências

Testes Rust cobrem framing fragmentado, limites, UTF-16, versão/URI, caminhos,
links, autorização e negação de efeitos. A jornada nativa utiliza o servidor real
para revisar permissões, editar um rascunho sem gravá-lo, navegar, corrigir,
reiniciar e revogar a análise. Uma configuração que tenta ativar build scripts é
mascarada na fixture, e a ausência do marcador de `build.rs` é verificada.

- [Configuração oficial do rust-analyzer](https://rust-analyzer.github.io/book/configuration)
- [Precedência de configuração na versão testada](https://github.com/rust-lang/rust-analyzer/blob/2026-09-21/crates/rust-analyzer/src/config.rs)
- [LSP 3.17](https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/)
