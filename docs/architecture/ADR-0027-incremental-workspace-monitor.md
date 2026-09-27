# ADR-0027 — Monitor incremental do workspace no Linux

Status: implementada localmente em 2026-09-27.

## Problema e decisão

A ADR-0025 percorria até 5 mil entradas e relia todas as fontes Rust a cada
1,5 segundo. O Explorer repetia a listagem completa após qualquer mudança de
conteúdo. Isso limitava projetos maiores e gerava I/O mesmo com o projeto parado.

O backend passa a manter uma árvore de metadados e uma instância inotify por
workspace. A primeira consulta/listagem enumera a árvore e registra os diretórios
antes de ler seus filhos. Consultas seguintes drenam eventos não bloqueantes e
verificam somente caminhos afetados e abas abertas. Não há thread permanente.
`libc`, já presente no projeto, fornece as chamadas; nenhuma dependência foi adicionada.

Criação/remoção de arquivo altera apenas sua entrada. Eventos de diretórios
invalidam e reconstroem as subárvores afetadas. Todos os prefixos antigos são
desregistrados antes de observar os novos, permitindo rename em qualquer ordem
lexicográfica. Eventos de descritores removidos são descartados. Uma pasta movida
para fora não passa a autorizar leitura externa: cada consulta parte da raiz
aprovada e abre componentes sem seguir links, usando descritores fixados.

A raiz é revalidada por identidade a cada consulta. Perda da fila, substituição
da raiz ou erro de observação exige reconstrução e ressincronização. Uma auditoria
completa de metadados a cada 60 segundos também cobre mudanças não notificadas.
A auditoria sem diferença não reinicia Rust nem recarrega abas. Mudanças de conteúdo
notificadas por IN_MODIFY são preservadas mesmo se os timestamps forem iguais.

## Limites e compatibilidade

- Até 100 mil entradas visitadas, 64 níveis e 16 MiB somando caminhos; cada caminho
  tem até 4.096 bytes. A resposta de listagem usa o mesmo cache do monitor.
- Até 8.192 watches de diretório e uma fila drenada de até 2 MiB por consulta.
- Até 128 abas informadas ao monitor; arquivos abertos de até 2 MiB mantêm SHA-256,
  com até 32 MiB de leitura por consulta. Isso inclui abas em target/node_modules.
  Fontes fechadas são acompanhadas por eventos/metadados, sem releitura periódica
  de conteúdo. Abrir uma aba inalterada não gera invalidação da análise.
- `.git`, `target` e `node_modules` são omitidos da árvore observada. Symlinks não
  são percorridos; links e arquivos especiais não aparecem no Explorer.

Se inotify não puder iniciar ou completar todos os watches, os watches parciais
são liberados e o snapshot completo passa ao modo de compatibilidade. A interface
informa esse modo e consulta a cada 5 segundos; o backend tenta voltar aos eventos
nas próximas reconstruções. Overflow libera a instância antes de refazer os
watches, evitando duplicar o consumo da cota do sistema.

Erros e limites preservam o último snapshot publicado. Atualizações são preparadas
em um lote e validadas antes de alterar a árvore; após erro, a próxima consulta
reconstrói a observação. Trocar projeto e destruir a janela libera toda a instância
e suas watches, sem depender de uma nova consulta.

## Interface, análise e segurança

O contrato mantém token, replay do último lote e ressincronização para cursor
antigo. Acrescenta `treeChanged` e `mode`. O frontend só recarrega o Explorer
quando há mudança estrutural ou ressincronização; conteúdo atualiza abas, busca
ativa e status Git. A listagem é executada no pool bloqueante, vinculada ao projeto
atual, e preserva a ordem de diretórios antes de arquivos. Um refresh manual pode
avançar o cache; o próximo poll ainda recebe o lote ou uma ressincronização.

Reconciliação de rascunhos, conflitos, CAS ao salvar, backup e versões de consultas
LSP seguem as ADRs 0020–0025. Rust continua reiniciando por lote de alterações em
suas fontes/configuração; isto não implementa atualização incremental do VFS do
rust-analyzer. Mudanças apenas de conteúdo não Rust não reiniciam a análise.
Nenhum arquivo é escrito pelo monitor e não há leitura de diretórios externos de
toolchains/cache.

## Validação e limites restantes

Testes cobrem 12 mil arquivos: uma consulta ociosa não percorre entradas e uma
edição isolada verifica um caminho, além da verificação da raiz. Também cobrem
subárvores movidas para dentro/fora, novos descendentes, troca por symlink,
mtime restaurado, abas em pastas ignoradas, auditoria, replay, limites, quota de
watches e overflow injetado no descritor de eventos. A jornada nativa acrescenta
6 mil arquivos, Explorer completo, recarga de aba limpa sem reconstruir o DOM da
árvore, preservação de rascunho e rename/remoção de diretório.

A matriz de filesystems remotos, mmap, bind mounts e alterações que preservem
metadados de fontes fechadas não está homologada. A auditoria é de metadados e
não garante detectar conteúdo fechado modificado sem evento e com metadados
idênticos. A árvore da UI ainda não é virtualizada. Busca de conteúdo e operações
LSP continuam com seus próprios limites: ampliar a observação não remove os
limites de 5 mil entradas na busca ou 512 fontes na preparação de edições Rust.

Referências: [inotify(7)](https://man7.org/linux/man-pages/man7/inotify.7.html) e
[inotify_add_watch(2)](https://man7.org/linux/man-pages/man2/inotify_add_watch.2.html).
