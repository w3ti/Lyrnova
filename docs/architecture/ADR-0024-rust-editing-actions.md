# ADR-0024 — Autocomplete, referências, renomeação e formatação Rust

Status: implementada localmente em 2026-09-25.

O editor amplia a sessão isolada de rust-analyzer das ADRs 0021–0023 com quatro
pedidos tipados: completion, references, rename e formatting. Cada recurso depende
da capacidade anunciada pelo servidor. Continuam valendo autorização do plugin,
workspace/sessão, versões, cancelamento, limite de oito consultas e timeout de cinco
segundos. O status também informa versões aceitas dos documentos: diagnósticos
iguais podem não ser publicados novamente pelo servidor.

Autocomplete usa um provider Monaco adicional aos snippets existentes. São aceitos
texto, snippets, ranges UTF-16 e edições adicionais do mesmo documento, sem
sobreposição. Até 256 sugestões por resposta, com indicação de lista incompleta.
Documentação Markdown e comandos do servidor não são repassados; itens que exigem
comandos são descartados. Auto-import está desativado; completion resolve não faz
parte desta entrega.

Shift+F12 e o menu de contexto abrem uma lista navegável de referências, incluindo
a declaração. Até 256 localizações, com a mesma validação de caminhos/ranges de
F12. A seleção verifica se o resultado continua atual antes de navegar; bibliotecas
externas continuam no visualizador somente leitura. Fechar/revogar cancela a consulta.

F2 retorna apenas edições de texto em arquivos Rust do workspace. WorkspaceEdit
aceita changes ou documentChanges; versões incompatíveis, operações de arquivos,
anotações que exigem confirmação, caminhos externos, symlinks e ranges inválidos
são rejeitados. Uma resposta inteira falha se qualquer arquivo/edição for inválido.
Antes da consulta, fontes fechadas são lidas sem seguir symlinks e comparadas ao
resultado: até 512 fontes / 8 MiB e 50 mil entradas visitadas, ignorando .git,
target e node_modules. Esses limites recusam renomeações em workspaces maiores.
Atualização: a [ADR-0025](ADR-0025-workspace-external-changes.md) acrescenta
detecção periódica e invalidação da análise. Ainda existe um intervalo entre
a alteração no disco e sua observação: o servidor ainda pode ter uma
visão antiga do disco antes do início da consulta; reiniciar análise recarrega a base.

O frontend lê todos os destinos faltantes antes de preparar qualquer modelo, confere
o texto original e a sessão, preserva rascunhos e recusa conflitos de recuperação.
Arquivos fechados viram abas com a revisão original do disco. O WorkspaceEdit do
Monaco inclui versionId e valida todos os modelos antes de aplicar. As alterações
ficam em memória, participam do backup de sessão e do desfazer **por arquivo**.
Salvar continua explícito e verifica a revisão do disco. Não há gravação LSP nem
novo grant de workspace_write ao plugin. Limite de 32 abas na preparação e 2.048
edições por arquivo; texto de entrada mais inserções não pode exceder 512 KiB.

Shift+Alt+F usa textDocument/formatting. RUSTFMT aponta para bin/rustfmt da toolchain
selecionada; rust-analyzer fornece a edição Rust correspondente ao crate. Ausência
do componente/falha de sintaxe não modifica o documento. rustfmt roda dentro do
mesmo sandbox sem rede e sem escrita no workspace. Configuração rustfmt do projeto
é lida normalmente; overrides de execução do rust-analyzer continuam mascarados.
Não há format on save, formatação de seleção nem download de ferramentas.

Testes cobrem ranges/surrogates, sobreposições, comandos, recursos externos,
versões e arquivos fechados alterados durante consultas; no frontend, cancelamento,
conflitos e preparação integral. A jornada nativa exercita os quatro recursos com
rust-analyzer/rustfmt reais e atalhos do Monaco, verifica rascunhos, disco e desfazer.

Referências de implementação:
[LSP 3.17](https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/),
[execução de rustfmt no rust-analyzer](https://github.com/rust-lang/rust-analyzer/blob/master/crates/rust-analyzer/src/handlers/request.rs),
e os contratos locais de Monaco 0.53 em `ui/node_modules/monaco-editor/monaco.d.ts`.
