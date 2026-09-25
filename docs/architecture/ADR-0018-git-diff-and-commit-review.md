# ADR-0018: Diff real e commit vinculado à revisão

- Status: implementada localmente
- Data: 2026-09-25

O status Git já era real, mas o inspector apresentava um diff demonstrativo e o
commit não exigia revisar o conteúdo preparado. O fluxo agora consulta diferenças
reais e exige uma revisão criada pelo núcleo antes de confirmar um commit local.

## Contrato

- `git_diff` recebe um path relativo presente no status e o escopo fechado
  `worktree` ou `index`. Não recebe comandos, referências ou opções arbitrárias.
- Paths viram pathspecs literais. Renames preparados incluem origem e destino;
  unstage de rename restaura ambos. Arquivos excluídos também são selecionáveis.
- Arquivos novos usam a leitura segura do workspace. Diffs rastreados vêm do Git,
  sem drivers externos nem textconv; symlinks versionados mostram o alvo textual,
  não o conteúdo do destino. Ancestrais symlink são recusados no diff de worktree.
- Git fica fora da thread da interface. Captura de stdout/stderr é limitada;
  cada comando tem timeout de 15 segundos e, em Unix, grupo próprio para cleanup.
  Stderr não cruza o IPC. Variáveis herdadas `GIT_*` não podem redirecionar a operação.
- O preview limita a saída a 1 MiB e 12.000 linhas, informa truncamento e nunca
  interpreta conteúdo como HTML. Texto fora de UTF-8 é recusado; binários aparecem
  como resumo. Monaco oferece rolagem virtualizada, seleção/cópia e sinais `+`/`-`
  além de cores, nos temas claro, escuro e alto contraste.
- Respostas atrasadas não podem substituir a seleção mais recente. Atualização
  manual, foco da janela e mutações locais recarregam status/diff; não há watcher
  contínuo de mudanças externas nesta entrega.

## Revisão e confirmação

`git_commit_review` valida a mensagem e captura árvore do índice (`write-tree`),
HEAD completo e referência atual. O diff integral é calculado entre objetos
imutáveis, incluindo o caso de primeiro commit sem HEAD. O núcleo guarda uma única
revisão por instância do projeto com mensagem, snapshot, UUID e validade de cinco
minutos. Criar uma revisão pode gravar objetos/cache internos do Git; não modifica
arquivos versionados nem faz commit.

O diálogo mostra branch, quantidade real de arquivos, mensagem e diff preparado.
Cancelar descarta o token. Confirmar envia somente esse token; o frontend não
reenvia a mensagem ou escolhe a árvore a publicar. Token expirado, reutilizado,
cancelado ou pertencente a outro projeto é recusado. O fechamento do projeto
descarta sua instância de revisão; o processo não persiste tokens.

O núcleo revalida índice, HEAD e referência, cria o commit com `commit-tree` sobre
a árvore revisada e revalida novamente antes de atualizar a referência exata com
`update-ref` e comparação do valor anterior. Conteúdo preparado concorrentemente
nunca entra no commit nem é sobrescrito. Falha após criar o objeto pode deixá-lo
inalcançável para a coleta normal do Git; não publica a referência. Falha ao buscar
status depois de um commit bem-sucedido não transforma o resultado em falha de commit.

Hooks e assinatura continuam desativados, agora declarados no diálogo. O fluxo
simples recusa conflitos e operações de merge/rebase/cherry-pick/revert em andamento,
que exigem semântica própria de pais, mensagens e continuidade. Revisão truncada
ou não textual não concede token; o usuário precisa reduzir o conjunto preparado
ou concluir pelo terminal.

## Limites

A comparação de referências evita sobrescrever avanço concorrente. Uma troca de
checkout exatamente após a última revalidação ainda pode fazer a atualização
atingir a branch originalmente revisada, que é o destino exibido e autorizado.
O índice continua intacto. Não há promessa de transação global entre HEAD,
checkout, índice e editores externos.

Permanece o risco de troca concorrente de ancestrais no filesystem registrado na
ADR-0015. Diffs de base arbitrária, último turno, multirrepositório, review inline,
revert e push ficam para entregas posteriores.

## Validação

Os testes usam repositórios temporários e cobrem primeiro commit, separação
staged/unstaged, replay/cancelamento/expiração, troca de branch/HEAD/índice, nomes
Unicode/pathspec, rename/delete, binários, conteúdo não UTF-8, limites, conflitos,
metadados, submodules e symlinks. O smoke em `ui/tests/git-review-smoke.py` executa
o bundle real em WebKitGTK com IPC sintético e cobre escopos, respostas atrasadas,
cancelamento, conteúdo HTML como texto, revisão invalidada e confirmação por token.

Referências: [git-diff](https://git-scm.com/docs/git-diff),
[git-commit-tree](https://git-scm.com/docs/git-commit-tree) e
[git-update-ref](https://git-scm.com/docs/git-update-ref).
