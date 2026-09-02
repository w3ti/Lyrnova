# ADR-0016: Broker de processos com sandbox e lifecycle cancelável

- Status: aceita
- Data: 2026-09-02

## Contexto

Comandos sugeridos por agentes, plugins e Tasks não podem herdar a autoridade do
processo principal do IDE. Mesmo uma execução aparentemente somente leitura pode
vazar environment, acessar paths externos, abrir rede, produzir saída ilimitada ou
deixar descendentes vivos depois de um cancelamento.

## Decisão

O núcleo passa a preparar toda execução em duas fases. `review` valida origem,
workspace, cwd, environment, comando, timeout e autoridade antes de qualquer efeito,
e devolve um token opaco de uso único ligado ao plano imutável. `execute` consome
esse token; revisões expiram em cinco minutos e podem ser descartadas.

Comandos usam `program + args` estruturados por padrão. Shell é uma variante
explícita e seu script é exibido byte por byte na revisão. A representação visual de
argv usa quoting apenas para leitura humana e nunca volta a ser interpretada pelo
shell. Operações de escrita, rede, host e termos destrutivos recebem risco elevado;
as três autoridades são verificadas independentemente.

No Linux, `read_only` e `workspace_write` exigem Bubblewrap funcional. O sandbox:

- cria namespaces, remove capabilities e nega rede por padrão;
- monta somente toolchain/sistema essenciais como leitura e o workspace em `/workspace`;
- deixa HOME e `/tmp` privados e parte de environment vazio com uma allowlist curta;
- limita arquivos abertos, tamanho de arquivo, memória virtual, core dumps e forks;
- executa em grupo próprio para timeout, cancelamento e limpeza de descendentes.

Se o isolamento solicitado não estiver disponível, a execução falha fechada. O modo
`escalated` é uma categoria separada, nunca um fallback automático; ele exige
autoridade explícita e ainda mantém cwd, environment limpo, limites, streaming e
lifecycle do broker.

Stdout e stderr são drenados concorrentemente para evitar deadlock, emitidos como
texto não confiável e capturados somente até 1 MiB por stream. O restante continua
sendo drenado sem retenção. Timeout e cancelamento enviam `SIGTERM` ao grupo e, após
uma janela curta, `SIGKILL`. Eventos de auditoria guardam origem, política, fase,
resultado e SHA-256 da apresentação do comando, nunca o comando ou environment.

## Consequências

A fronteira é independente do provider e pode atender agentes, plugins e Tasks sem
dar a nenhum deles acesso direto a `Command`. A disponibilidade real de sandbox é
diagnosticável separadamente para rede isolada e compartilhada.

O terminal interativo continua separado por representar uma ação direta do usuário.
Tasks de runtimes externos usam um catálogo estritamente tipado: a seleção contém
somente plugin e ID, o Rust consulta novamente a definição, deriva
`ProcessAuthority` dos grants persistidos e entrega à interface um token opaco para
revisão. O conjunto exato de grants é revalidado antes do consumo; mudança de
workspace ou lifecycle do plugin invalida revisões e cancela execuções associadas.

## Riscos residuais

`RLIMIT_NPROC` é aplicado por usuário no Linux, não como um contador exclusivo da
Task. Antes da auditoria v1.0, cgroup v2 deve substituir ou complementar esse limite
para quotas e limpeza fortes por execução, especialmente no modo escalated. A
resolução de cwd também deverá migrar para handles (`openat2`/`fchdir`) para fechar a
janela TOCTOU remanescente entre revisão e spawn.
