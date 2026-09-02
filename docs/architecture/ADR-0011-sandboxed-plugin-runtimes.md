# ADR-0011: Executar runtimes externos em sandbox

- Status: aceito
- Data: 2026-09-02

## Contexto

Um pacote validado continua sendo código não confiável. Tornar seu entrypoint
executável diretamente no host permitiria acessar o filesystem, ambiente, rede
e processos fora das concessões revisadas pelo usuário. Habilitação persistida,
troca de workspace e remoção também precisam participar do lifecycle.

## Decisão

No Linux, runtimes `process` externos são iniciados exclusivamente pelo broker
Rust usando Bubblewrap em caminho fixo. A ausência do sandbox falha fechada e
impede a habilitação. Outras plataformas recusam runtimes externos até receberem
um backend com garantias equivalentes.

O broker repete a decisão de política a partir das concessões persistidas. Ele
revalida recibo e árvore instalada, exige `process_spawn`, limpa o ambiente,
remove capabilities, cria namespaces de
usuário, PID, IPC, UTS, cgroup e rede, e compartilha a rede somente quando
`network_access` foi concedida. O pacote é montado somente para leitura. Apenas o
workspace ativo é montado em `/workspace`: somente leitura para `workspace_read`
e leitura/escrita para `workspace_write`. Sem essas permissões, o path existe mas
fica vazio. Secret storage e approvals permanecem serviços mediados pelo host e
nunca viram mounts ou variáveis de ambiente.

O entrypoint instalado continua sem bit de execução. A cada sessão, o núcleo cria
uma cópia privada e executável e a sobrepõe no mesmo path dentro do mount
read-only. O processo recebe apenas identificadores públicos do protocolo, limites
de arquivos abertos, tamanho de arquivo e memória virtual, além de `no_new_privs`.
Entrada, saída e erro ficam desconectados até que o transporte do protocolo v1
seja ligado a superfícies específicas do host; habilitar não cria um command de
shell ou canal genérico no frontend.

Ativar inicia o processo antes de persistir a habilitação e reverte o processo se
a persistência falhar. Desativar, remover, trocar de workspace ou encerrar o app
termina o runtime e limpa sua sessão. No reinício, resíduos são removidos e apenas
plugins externos ainda habilitados são reiniciados; falha no sandbox os desabilita.

## Consequências

- conteúdo de pacote nunca é executado diretamente no host;
- rede e workspace materializam exatamente a política concedida;
- o plugin não recebe HOME, secrets, ambiente do IDE ou filesystem do usuário
  fora do workspace concedido;
- Bubblewrap passa a ser dependência de runtime do pacote Linux;
- o transporte funcional do protocolo deverá usar pipes estruturados e limites,
  sem ampliar os mounts ou expor execução genérica.
