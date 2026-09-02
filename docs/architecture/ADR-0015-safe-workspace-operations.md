# ADR-0015: Operações seguras e recuperáveis no workspace

- Status: aceita para o MVP Linux
- Data: 2026-09-02

## Contexto

Listar, abrir e salvar arquivos existentes não cobre o fluxo básico de um IDE.
Busca, criação, patch, movimentação e exclusão também recebem paths e conteúdo não
confiáveis e precisam preservar a raiz autorizada, mudanças externas e a capacidade
do usuário de cancelar ou recuperar uma ação destrutiva.

## Decisão

O `WorkspaceService` continua sendo a única fronteira de filesystem. Todas as APIs
recebem paths relativos portáveis e os validam novamente imediatamente antes do
efeito. Paths absolutos, NUL, controles, barras invertidas, prefixos de drive,
segmentos vazios, `.` e `..` são recusados. Entradas existentes são canonicalizadas
sob a raiz e qualquer symlink encontrado no caminho encerra a operação. Cada
componente também respeita limite de 255 bytes, caracteres reservados, sufixos e
nomes de dispositivo do Windows para que uma árvore criada no Linux seja portável.

As operações têm estes contratos:

- a listagem ignora `.git`, `target` e `node_modules` e para em 5.000 entradas;
- metadados informam tipo, tamanho, modificação e classificação binária limitada;
- leitura e escrita aceitam somente arquivo regular UTF-8, sem NUL, até 2 MiB;
  leitura parcial limita cada faixa a 256 KiB e nunca divide um caractere UTF-8;
- busca percorre apenas a listagem autorizada, limita consulta a 256 bytes, retorno
  a 200 ocorrências e leitura total a 32 MiB; binários e arquivos grandes são
  ignorados;
- criação usa `create_new` e movimentação usa rename sem substituição no Linux;
- salvamento e patch exigem revisão SHA-256; patches são ordenados, não sobrepostos,
  respeitam limites UTF-8 e comparam exatamente o trecho esperado. Uma operação
  separada produz conteúdo e revisão de preview sem escrever;
- line endings não são normalizados: leitura, salvamento e patch preservam os bytes
  não alterados;
- exclusão só ocorre depois da confirmação explícita do frontend. O núcleo move a
  entrada, por rename, para uma área privada do aplicativo e devolve um token opaco;
- o token permite restauração durante a sessão, sem revelar o path da área privada.
  Tickets de sessões encerradas são removidos na inicialização seguinte;
- toda mutação bem-sucedida emite `workspace-mutated`, atribuído a `local_user`, com
  UUID, operação, paths e revisões anterior/posterior quando disponíveis.

O Explorer permite selecionar, criar, mover/renomear e excluir. Uma operação que
afetaria um rascunho não salvo é recusada. Cancelar um prompt ou a confirmação não
invoca o command de escrita. A busca de nome e conteúdo é executada no núcleo, com
debounce e descarte de respostas antigas no frontend.

## Consequências

A exclusão é recuperável sem depender da lixeira específica do desktop e não cruza
filesystem silenciosamente: se a área privada não aceitar rename atômico, a operação
falha antes de remover a origem. A restauração falha sem sobrescrever uma entrada que
tenha surgido no mesmo path.

A recuperação é deliberadamente efêmera, não um sistema de backup. Arquivos grandes
e binários podem ser listados, movidos e excluídos, mas nunca são enviados ao editor
como texto. O limite de busca pode omitir ocorrências depois de esgotar o orçamento.

## Riscos residuais

O Linux usa `renameat2(RENAME_NOREPLACE)` para impedir substituição no efeito final.
Ainda será necessário migrar toda a resolução para handles de diretório (`openat2`
ou equivalente) antes da auditoria v1.0 para eliminar a janela residual entre a
validação de um ancestral e uma troca concorrente desse diretório. Os testes atuais
cobrem traversal gerado, symlink estático e a substituição do pai depois da criação
do serviço, além de conflitos de revisão e precondição de patch.
