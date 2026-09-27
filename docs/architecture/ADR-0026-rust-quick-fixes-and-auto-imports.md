# ADR-0026 — Correções rápidas e auto-imports Rust

Status: implementada localmente em 2026-09-25.

A sessão isolada da ADR-0024 passa a aceitar `code_action` e
`completion_resolve` no protocolo fechado de consultas. Ambos exigem as
capacidades anunciadas pelo servidor e mantêm autorização, sessão, versão,
cancelamento e limites existentes. Não há execução genérica de comandos nem
`workspace/applyEdit` autorizado.

## Correções rápidas

O provider Monaco oferece Ctrl+. e a lâmpada de correções. O pedido LSP contém
range UTF-16 validado e `context.only = ["quickfix"]`. Anunciamos suporte a
CodeAction literal, sem resolução tardia de ações; o servidor precisa devolver
as edições completas. Apenas ações quickfix com edições de texto são oferecidas.
Ações desabilitadas, comandos, operações de criação/rename/delete, alterações
anotadas, destinos externos ou edições sobrepostas são descartados por ação.
Até 32 ações e 8 MiB de originais/inserções são devolvidos por resposta.

O validador de WorkspaceEdit é compartilhado com renomeação. Arquivos fechados
são observados antes da consulta e revalidados ao receber a resposta. Antes da
seleção nenhuma aba ou rascunho é criado. Ao selecionar, o frontend confere
novamente sessão, conteúdo, conflitos e cancelamento, prepara todos os modelos e
entrega edições versionadas ao Monaco. O salvamento permanece explícito;
Ctrl+Z desfaz por arquivo. Os limites de 512 fontes / 8 MiB para o levantamento
e 32 abas preparadas continuam aplicáveis. A janela de detecção do monitor da
ADR-0025 permanece; este trabalho não implementa um monitor incremental.

## Auto-imports

A configuração ativa `completion.autoimport.enable` e anuncia
`additionalTextEdits` e `detail` em `completionItem.resolveSupport.properties`.
`detail` é validado e apresentado como texto simples. O rust-analyzer fixado
só anuncia resolveProvider quando há também uma propriedade como detail;
apenas additionalTextEdits ativa auto-imports sem anunciar resolução.
O backend guarda os objetos de completion recebidos do servidor e entrega
identificadores UUID opacos ao frontend. A resolução só aceita um desses IDs,
no mesmo documento, posição, versão e revisão da sessão. O cache mantém uma
única lista (até 256 itens / 2 MiB), expira em 120 segundos e é revogado ao
parar a sessão; mudanças de fontes/rascunhos invalidam sua revisão.
Dados brutos do servidor nunca atravessam o IPC de resposta.

A resposta de resolução não pode mudar label, inserção, range, formato de snippet
ou comando apresentados originalmente. Os imports adicionais passam pelo mesmo
validador UTF-16, sem sobreposição entre si ou com a inserção principal.

Monaco 0.53 pode aceitar Enter antes de concluir uma resolução tardia. Para
preservar a inserção conjunta de símbolo e import, resolvemos os candidatos
antes de oferecer a lista. São até 32 candidatos com resolução e quatro
consultas simultâneas; os demais ficam de fora e a lista é marcada incompleta,
permitindo refinar a busca digitando mais. Falha ou cancelamento não oferece
uma sugestão que precise de import incompleto. Isso acrescenta latência à lista,
mas permite aceitação imediata e um único Ctrl+Z para símbolo e import.
Sugestões sem resolução mantêm o limite geral de 256 itens.

## Validação e escopo

Testes cobrem IDs desconhecidos/expirados, revisão alterada, stop, inserção
adulterada na resolução, imports sobrepostos, comandos, caminhos externos,
operações de arquivos e arquivo fechado alterado durante a consulta. Providers
JavaScript verificam preparação apenas na seleção, resultados obsoletos,
cancelamento e limite de concorrência. A jornada nativa verifica importação via
Ctrl+Espaço e Ctrl+., desfazer e disco inalterado com rust-analyzer real.

Não inclui refatorações gerais, organização de imports, fixes que criam arquivos,
execução de comandos do servidor, Cargo check/clippy ou correções baseadas em
build scripts/proc macros. Dependências e stdlib seguem o ambiente autorizado.

Referências: [auto-import no rust-analyzer](https://rust-analyzer.github.io/book/features.html#auto-import),
[configuração oficial](https://rust-analyzer.github.io/book/configuration#rust-analyzer.completion.autoimport.enable),
[LSP 3.17](https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/)
e os contratos/fontes locais de Monaco 0.53.
