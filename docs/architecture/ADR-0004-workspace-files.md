# ADR-0004: Fronteira segura para edição manual de arquivos

- Status: substituída pela ADR-0015
- Data: 2026-09-01

## Contexto

O editor precisa abrir e salvar arquivos do projeto sem transformar o frontend
em uma API genérica de filesystem. Paths, conteúdo e estado do arquivo são
dados não confiáveis e podem mudar enquanto uma aba está aberta.

## Decisão

Toda leitura e escrita passa pelo `WorkspaceService` Rust. O frontend envia um
path relativo e recebe conteúdo UTF-8 mais uma revisão SHA-256. Ao salvar,
precisa devolver a revisão lida; se o conteúdo no disco mudou, a operação falha
com `conflict` e preserva o rascunho do editor.

Controles atuais:

- apenas paths relativos formados por componentes normais;
- rejeição de path absoluto, `..`, NUL e arquivo inexistente;
- canonicalização e verificação contra a raiz autorizada;
- rejeição de symlink em qualquer componente;
- somente arquivos regulares UTF-8 de até 2 MiB;
- somente edição de arquivo existente; criação e exclusão não estavam expostas
  nesta etapa inicial;
- escrita em arquivo temporário no mesmo diretório, `sync_all`, preservação de
  permissões e rename atômico no Linux;
- `.git`, `target` e `node_modules` não entram no explorer;
- máximo de 5.000 entradas por listagem.

## Seleção da raiz

Criar e abrir projetos usa o seletor nativo de diretórios. O núcleo troca a
raiz autorizada somente depois de canonicalizar a seleção e reinicia o terminal
para que nenhum processo continue ligado ao workspace anterior. O último
projeto válido é persistido numa lista local limitada e reaberto na próxima
inicialização. Entradas ausentes ou inválidas são ignoradas; sem uma raiz
válida, o aplicativo inicia no estado “Nenhum projeto” e não concede acesso ao
diretório home, `/` ou ao diretório de lançamento.

## Evolução

A ADR-0015 amplia esta fronteira com busca, metadados, criação, patch,
movimentação, exclusão recuperável, eventos de mutação e os limites atuais.
