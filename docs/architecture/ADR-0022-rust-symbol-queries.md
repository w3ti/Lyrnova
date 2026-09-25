# ADR-0022 — Definição e hover para Rust

- Data: 2026-09-25
- Estado: implementado localmente, validação registrada no checkpoint

A [ADR-0023](ADR-0023-rust-toolchains-and-local-dependencies.md) amplia esta entrega
com seleção de toolchains e fontes externas revisadas; as limitações abaixo
registram o escopo original de definição/hover.

## Comportamento

O plugin Rust autorizado registra os providers de definição e hover do Monaco.
F12 navega até a definição, inclusive abrindo outra aba do projeto. Hover mostra
tipos e documentação; também pode ser solicitado por Ctrl+K, Ctrl+I. São recursos
de leitura, usando o mesmo servidor isolado e os mesmos grants da ADR-0021.
Não há novas permissões nem instalação automática de executáveis.

O escopo continua sendo fontes Rust locais. Biblioteca padrão, dependências
externas, proc macros, build scripts, completion LSP, referências, rename e
formatação não são integrados por esta entrega. Não há consumidor de comandos
ou edições enviados pelo servidor. O hover é texto literal em bloco de código,
sem links, imagens, HTML ou comandos acionáveis, mesmo se o servidor responder
com Markdown legado. Pré-visualização de definições em arquivos sem modelo
carregado não faz parte do aceite; F12 abre esses arquivos pelo fluxo do editor.

## Consultas e lifecycle

`language_query` recebe uma enumeração fechada (`hover` ou `definition`), UUID de
requisição, sessão, path de documento aberto, versão e posição UTF-16. A autorização
é revalidada antes de enfileirar e antes de consumir o resultado. Não se mantêm os
locks de plugin/workspace durante a espera pelo processo. Troca de workspace,
reinício ou revogação impedem a entrega do resultado na sessão seguinte.

Há até oito consultas pendentes por servidor, com prazo de cinco segundos. IDs
LSP são monotônicos e separados de initialize/shutdown; respostas fora de ordem
são correlacionadas e respostas de consultas removidas são ignoradas. O servidor
precisa anunciar suporte ao recurso. A fila envia didOpen/didChange/didClose antes
da consulta correspondente, mantendo a mesma revisão sob lock ao construir as
mensagens. Qualquer mudança no conjunto de documentos invalida as consultas
anteriores, inclusive alterações em uma aba diferente da origem.

O frontend descarta resultados após edição, fechamento, cancelamento do Monaco,
reinício ou desativação, e pede cancelamento ao backend. O worker encaminha
`$/cancelRequest` quando uma consulta enviada é cancelada, fica obsoleta ou expira.
Cancelamento é cooperativo; timeout e validação de geração/versão continuam
necessários mesmo que o servidor ignore a notificação. Uma corrida entre chegada
do cancelamento e enfileiramento não concede validade ao resultado na UI.

## Validação dos resultados

Hover aceita os formatos de conteúdo previstos no LSP, limitados a 32 partes e
64 KiB. Range opcional precisa caber no texto solicitado. Posições não podem
cortar pares substitutos UTF-16. O frontend converte o conteúdo inteiro em texto
inerte, normalizando quebras de linha antes de delimitar o bloco de código.

Definições aceitam Location, Location[] e LocationLink[], até 32 resultados. URIs
precisam ser file URIs canônicas sob `/workspace`, sem query/fragmento, traversal,
`.git` ou symlinks, e apontar para `.rs`. Ranges são validados contra os rascunhos
abertos ou a leitura limitada do arquivo alvo. LocationLink usa targetSelectionRange,
contido em targetRange. Arquivos não abertos têm limite de 512 KiB cada e 8 MiB
no total por resposta. A leitura de validação percorre descritores de diretórios
com openat/O_NOFOLLOW e só aceita arquivo regular, sem bloquear em FIFOs.

A navegação usa o fluxo existente de leitura e recuperação do editor, preservando
rascunhos. As garantias e riscos residuais de mutação externa desse fluxo continuam
os da fronteira de workspace: o resultado LSP não é um snapshot transacional do
filesystem e uma alteração externa posterior pode deslocar o símbolo.

## Ferramentas de sistema e CI

A análise inicial requer `/usr/bin/cargo` e `/usr/bin/rustc` executáveis dentro de
`/usr`; um rustup instalado somente no HOME não está exposto no sandbox. A ausência
dessas ferramentas agora gera `toolchain_unavailable`, com orientação na UI.
O workflow instala os pacotes de sistema além do Rust usado para compilar o IDE.
Esse ajuste responde à falha do CI de `7f3413a` na espera de diagnósticos; sua
confirmação remota depende de executar o workflow com as alterações novas.

## Referências

- [LSP 3.17: hover](https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/#textDocument_hover)
- [LSP 3.17: definição](https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/#textDocument_definition)
- [ADR-0021](ADR-0021-rust-lsp-diagnostics.md)
