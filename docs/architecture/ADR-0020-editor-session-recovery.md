# ADR-0020 — Recuperação local da sessão do editor

- Data: 2026-09-25
- Estado: implementada localmente; publicação não realizada

## Problema

O IDE reabria o projeto recente e o primeiro arquivo da árvore, mas perdia abas,
posições e rascunhos. Restaurar apenas o texto com a revisão atual do disco poderia
permitir que um salvamento sobrescrevesse uma edição externa sem conflito.

## Decisão

Cada workspace canônico possui um snapshot JSON v1 em
`app_data_dir/editor-sessions-v1/<sha256-do-caminho>.json`. O snapshot contém a
ordem das abas, arquivo ativo, linha/coluna, rolagem horizontal/vertical e somente
o conteúdo dos documentos alterados, acompanhado da revisão original. Documentos
limpos são relidos do disco. Uma lista vazia representa uma sessão intencionalmente
sem abas; não dispara a abertura automática do primeiro arquivo.

O frontend agenda gravações a cada 500 ms de atividade, sem adiar indefinidamente
por digitação contínua. Todas as escritas entram numa fila; fechar normalmente ou
trocar projeto aguarda a última fotografia com a edição temporariamente bloqueada.
A barra diferencia `Sessão guardada` de `Salvo no disco`. Falhas ficam visíveis;
fechar após falha exige confirmação. Troca de projeto com rascunhos sem backup é
recusada, permitindo salvá-los ou copiá-los. O evento de fechamento do Tauri atende
tanto ao botão próprio quanto à solicitação do gerenciador de janelas. Seu helper
necessita da permissão explícita `core:window:allow-destroy` após o callback.

A recuperação não grava documentos no workspace. Rascunhos de arquivos alterados
ou indisponíveis são abertos com aviso, preservando a revisão anterior e bloqueando
o salvamento comum. `Salvar cópia…` cria um caminho novo usando o contrato existente,
sem substituir um arquivo. `Descartar rascunho e reler` exige confirmação; se a leitura
falhar, o rascunho continua disponível. Fechar uma aba descarta sua recuperação
conforme a preferência já existente de confirmação de rascunhos.

## Armazenamento e concorrência

- Diretório criado com modo 0700 e arquivos com 0600; conteúdo não vai a logs,
  localStorage, Git, plugins ou serviços remotos por este fluxo.
- Escrita em temporário exclusivo, `sync_all`, rename atômico e sincronização do
  diretório. A última versão completa fica disponível durante uma gravação.
- Lock por workspace e comparação do hash dos bytes anteriormente lidos impedem
  que duas instâncias substituam silenciosamente snapshots divergentes. O lock é
  liberado explicitamente, inclusive quando um spawn herdou brevemente o descritor.
- IPC exige que o workspace informado corresponda ao projeto ativo; mantém a trava
  de leitura do projeto durante a operação e executa I/O no pool bloqueante.
- Até 64 abas, 2 MiB por rascunho e 8 MiB de JSON total; leitura limitada a 8 MiB + 1.
  Paths absolutos, travessia, `.git`, duplicações, revisões inválidas e links no
  arquivo de snapshot são recusados. Paths dos documentos continuam sujeitos às
  verificações do `WorkspaceService` quando relidos/criados.
- Corrupção e versões desconhecidas preservam os bytes existentes e desabilitam
  a sobrescrita automática. O editor continua utilizável e indica a falha.

## Limites

O snapshot é texto privado, não um cofre criptografado. Processos do mesmo usuário
ou root podem lê-lo; rascunhos podem conter segredos. Há uma versão por projeto,
sem histórico, expiração global, limpeza segura de mídia ou sincronização. Um
crash durante a escrita pode deixar temporários privados. Corrupção/versão futura
não é reparada automaticamente: requer preservar/mover o arquivo fora do app e
reiniciar; não existe migração de um formato anterior de sessão.

Após encerramento abrupto, recupera-se a última gravação concluída, não a última
tecla ainda na fila (500 ms mais latência de I/O). A suíte reinicia o processo sem
fechamento normal depois de confirmar o snapshot, mas não simula queda de energia.
Seleções múltiplas, undo/redo, dobras, mudanças externas com merge automático e
workspaces renomeados não são restaurados. A implementação atual é Linux/Unix.
Shell, entrada/saída do terminal, Tasks e comandos nunca são reexecutados por esta
recuperação. Serviço de secrets do host, LSP/DAP e esses recursos continuam separados.

## Verificação

Testes Rust exercitam roundtrip, isolamento por workspace, permissões, conflitos,
locks herdados, limites, corrupção, schema futuro e symlinks. A jornada nativa usa
o backend real para editar, reiniciar, recuperar abas/cursor/rolagem, preservar
rascunhos de arquivos alterados/excluídos, copiar, reler e manter abas fechadas.
Também verifica o flush no fechamento e a indisponibilidade explícita da recuperação
quando o formato não é suportado. Entrada e respostas aos prompts são acionadas
pelo harness DOM; não homologam entrada do SO ou diálogos nativos.
