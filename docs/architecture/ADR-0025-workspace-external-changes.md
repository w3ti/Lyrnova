# ADR-0025 — Alterações externas no workspace

Status: implementada localmente em 2026-09-25.

Ações de Git, terminal e outras ferramentas podem modificar o disco enquanto o
editor mantém modelos e rascunhos. O CAS no salvamento já recusava sobrescritas,
mas o usuário precisava reler arquivos e reiniciar a análise manualmente.

## Detecção e escopo

O frontend consulta `workspace_poll` a cada 1,5 segundo, sem consultas sobrepostas,
e antecipa uma consulta ao recuperar foco ou concluir um salvamento. O backend
executa a varredura no pool bloqueante, vinculado ao projeto autorizado. Não há
thread permanente nem descritores de observação mantidos entre consultas.

A implementação Linux compara inode/dispositivo, tamanho, mtime e ctime de arquivos.
Fontes/configuração Rust e abas explicitamente abertas também recebem SHA-256,
mesmo com metadados iguais. A árvore ignora `.git`, `target` e `node_modules`;
arquivos explicitamente abertos nos dois últimos continuam acompanhados. Diretórios
não usam timestamps, evitando invalidar a análise só porque um build criou um cache.
Renomeações são observadas como remoção e criação de caminhos.

Limites por varredura: 5.000 entradas visitadas, 64 níveis, 128 caminhos de abas,
4.096 bytes por caminho e 32 MiB lidos para hashes. Arquivos acima do limite de
edição de 2 MiB são acompanhados por metadados. Erros/limites não publicam uma
árvore parcial nem descartam o snapshot anterior; a interface informa que a
atualização automática está indisponível e tenta novamente. Projetos maiores
precisam de suporte incremental futuro. Diretórios externos de toolchains/cache
não são observados. Alterações apenas internas ao índice/refs Git ficam fora deste
monitor; o status Git continua atualizando ao ganhar foco e após ações do IDE.

A varredura usa diretórios fixados por descritores e não segue symlinks. As leituras
de documentos também passaram a usar componentes `openat` com O_NOFOLLOW, leitura
limitada e O_NONBLOCK para evitar espera em um FIFO trocado por outro processo.
`workspace_read_current` valida o workspace sob lock de projeto antes de ler.

## Reconciliação com o editor

Cada snapshot possui token efêmero vinculado à raiz. A última diferença pode ser
reentregue; cursor desconhecido exige ressincronização completa das abas. O frontend
só reconhece o token após processar a atualização. Abas adiadas por salvamento ou
mudança de revisão permanecem pendentes, mesmo que nenhum novo evento ocorra.
Mudança de projeto, restauração e fechamento invalidam callbacks em andamento.

Para cada aba, a decisão é tomada **depois** da leitura, conferindo workspace,
identidade do modelo, revisão e salvamento pendente:

- aba limpa: recarrega conteúdo, atualiza revisão e preserva cursor/rolagem;
- rascunho divergente: mantém texto e revisão original, exibindo conflito antes de salvar;
- arquivo removido, substituído por symlink/binário ou incompatível: mantém a aba como
  rascunho recuperável, inclusive se antes estava limpa;
- conteúdo do disco igual ao rascunho, ou revisão original restaurada: reconhece o
  estado sem descartar texto digitado, limpando o conflito quando seguro;
- operação de salvar em andamento: adia e tenta novamente; a própria gravação do
  editor não é tratada como conflito externo.

Uma leitura atrasada nunca sobrescreve digitação recente nem outro modelo reaberto.
Falhas transitórias de IPC/leitura são repetidas. Avisos de conflito já presentes
não são apagados por uma nova observação idêntica. Recarregar explicitamente exige
o fluxo existente de descarte; salvar cópia e recuperação de sessão permanecem
ativos. Recarregar uma aba limpa substitui o modelo e reinicia seu histórico de undo;
a edição não salva nunca é recarregada automaticamente.

Explorer e resultados de busca são atualizados preservando pastas recolhidas. A
resposta mais recente prevalece, com proteção adicional por geração do projeto.

## Análise Rust

Mudanças em `.rs`, Cargo.toml/lock, configurações Cargo/Rust e ressincronizações
invalidam consultas pendentes e diagnósticos no backend. A sessão passa a `outdated`,
e resultados antigos deixam de ser aceitos. O frontend limpa referências/definições
em cache e reinicia a análise com os rascunhos atuais e o ambiente já autorizado.

Escolheu-se reiniciar por lote em vez de manter uma segunda tradução incremental
do filesystem para o VFS do rust-analyzer. Isso também refaz os mounts de configuração
e os metadados Cargo, sem baixar dependências nem executar build scripts. Salvar
uma fonte Rust pode provocar o mesmo reinício. Há latência de reindexação, sobretudo
com stdlib; atualização incremental e acompanhamento de diretórios externos são
trabalhos futuros. A análise continua limitada pelo ambiente offline já revisado.

## Validação

Testes Rust cobrem criação/remoção/rename, substituição atômica, conteúdo alterado
com mtime preservado, exclusões, symlinks, cursores, limites e invalidação LSP.
Testes JavaScript cobrem digitação durante leitura, saves pendentes, troca de projeto,
reabertura de abas, exclusão e reconciliação de conflitos. A jornada nativa usa
alterações reais em disco e confirma atualização de abas/Explorer, ausência de
sobrescrita de rascunhos e hover/definição atualizados após editar uma fonte fechada.
