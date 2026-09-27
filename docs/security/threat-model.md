# Threat model inicial do IDE e dos plugins

## Validação nativa e ciclo do terminal — 2026-09-25

A janela local recebe apenas `core:event:allow-listen`/`allow-unlisten` para
consumir streaming e auditoria; emitir eventos pelo frontend continua sem
permissão. A consulta do estado maximizado também é concedida ao controle de
janela existente. Essas permissões não ampliam os grants de plugins.

O terminal Linux usa PTY e uma sessão de processos própria. Reiniciar, trocar
projeto, descartar a sessão e destruir a janela principal encerram também os
grupos de jobs foreground/background antes de recolher o shell. Não é sandbox
nem contenção de processos que deliberadamente usem `setsid`; cgroups e crash
abrupto continuam fora da garantia. `waitid(WNOWAIT)` reserva o PID do líder até
a limpeza. Os membros da sessão são sinalizados por pidfds para evitar atingir
processos alheios por reutilização de PID. Start/stop não bloqueiam a thread de interface.

Entrada, resize, ACK e stop vindos do frontend exigem o identificador da sessão
atual. Há limites de filas e confirmação após parsing no xterm para que uma
saída contínua não cresça sem limite. Saída é uma sequência de bytes para o
emulador, nunca HTML; OSC de clipboard/links e operações de janela são desativados.
O contrato e limites estão na [ADR-0019](../architecture/ADR-0019-linux-pty-terminal.md).

A [jornada nativa](../../tests/e2e/README.md) exercita esses efeitos reais em um
perfil temporário, incluindo Tasks sob Bubblewrap, cancelamento de descendentes,
falha de plugin e conflitos de salvamento. Ela não substitui os testes negativos
de autoridade nem a homologação de entrada/acessibilidade do sistema operacional.

## Ativos

- código-fonte, Git e worktrees;
- arquivos externos ao projeto;
- credenciais, secret stores e variáveis;
- processos, rede e dispositivos;
- diagnósticos, templates, tasks, prompts opcionais, diffs e terminal;
- sessão de autenticação de plugins opcionais;
- integridade do aplicativo e dos pacotes.

## Atores e fronteiras

```text
Usuário
  │ escolhe projeto, modo e approvals
  ▼
Frontend local Tauri (não executa efeitos)
  │ eventos e commands tipados
  ▼
Núcleo Rust / Policy Engine
  ├── Host de plugins (código e intenção não confiáveis)
  │   ├── Linguagens / LSP / DAP / tasks
  │   └── Providers de IA opcionais
  ├── Ferramentas de workspace (roots limitados)
  ├── Broker de processos (sandbox)
  ├── Git / worktrees
  └── Persistência / secret store
```

Plugins, adapters, modelos, servidores de linguagem, toolchains e conteúdo do
repositório são tratados como não confiáveis para fins de autoridade.

## Ameaças prioritárias

| Ameaça | Controle |
| --- | --- |
| Path traversal ou symlink escapa do root | canonicalização, no-follow e validação próxima ao efeito |
| Busca esgota memória ou interpreta binário | limites de consulta/resultado/bytes; UTF-8 sem NUL |
| Mutação substitui entrada existente | criação exclusiva e rename sem substituição |
| Exclusão acidental perde dados | confirmação explícita, quarentena privada e token de restauração |
| Command injection | argumentos estruturados; shell somente explícito |
| Plugin tenta ler segredo | roots, deny rules e redaction |
| Exfiltração | rede negada por padrão e approval por destino |
| Approval reaproveitada | vínculo ao hash da ação exibida |
| Provider amplia “permitir na sessão” | regra local limitada à conversa + hash exato; provider recebe aceite pontual |
| Timeout ou janela fechada vira aceite | expiração e lifecycle sempre negam pendências |
| Patch sobre arquivo alterado | precondition/hash e conflito explícito |
| Diff executa código do repositório | `--no-ext-diff`, `--no-textconv`, hooks/fsmonitor desativados e argumentos literais |
| Commit inclui conteúdo não revisado | token efêmero ligado à árvore imutável, mensagem, HEAD e referência; revalidação e atualização condicional |
| Diff excessivo ou resposta atrasada engana a revisão | captura/tempo limitados, truncamento explícito, revisão incompleta sem token e descarte de respostas antigas |
| Processo continua após cancelamento | process group/job e cleanup |
| Processo inunda stdout/stderr | drenagem concorrente e captura limitada por stream |
| Fork bomb esgota o host | limite de processos; cgroup v2 planejado para quota por Task |
| Environment global vaza segredo | `env_clear` e allowlist pequena controlada pelo núcleo |
| Sandbox indisponível reduz proteção | falha fechada; host é modo escalated explícito, nunca fallback |
| ANSI/Markdown injeta UI | sanitização e CSP |
| Prompt injection em arquivo | conteúdo não amplia authority |
| Plugin/backend comprometido | protocolo limitado e policy engine independente |
| Provider assume identidade privilegiada | seleção por tipo/capabilities/grants; nenhum ID concede autoridade |
| Providers ativos concorrem pela UI | resolução ambígua falha fechada |
| Frontend injeta path de pacote | seleção nativa e path mantido somente no núcleo Rust |
| Confirmação troca permissões revisadas | token efêmero e igualdade exata no command Rust |
| Remoção promove versão antiga | quarentena atômica do diretório completo do ID |
| Falha após remoção física | rollback antes do commit; falha fechada se rollback falhar |
| Frontend troca URL ou hash de download | IPC aceita somente ID do catálogo embarcado |
| Redirect de release causa SSRF | HTTPS e allowlist exata verificados em cada salto |
| Download sem `Content-Length` esgota disco | limite aplicado durante o streaming |
| Catálogo tenta downgrade | versão comparada antes e depois da rede sob lock |
| Runtime externo escapa da autoridade | Bubblewrap obrigatório, mounts derivados dos grants e ambiente limpo |
| Plugin lê workspace sem concessão | `/workspace` vazio; bind somente read-only/read-write autorizado |
| Plugin continua após remoção ou troca de projeto | lifecycle central encerra namespace e limpa sessão |
| URL de login forjada | abrir somente HTTPS nos hosts permitidos pelo provider |
| Token vaza para o frontend/log | sessão no adapter; projeção mínima da conta; redaction |
| E-mail exposto indevidamente | memória/UI só quando necessário; nunca em telemetria |
| Corrupção/replay de eventos | IDs, reducer idempotente e persistência transacional |
| Supply chain | lockfiles, SBOM, assinatura e builds reproduzíveis |

O Monaco gera em tempo de execução a folha de cores usada pelos tokens. Por
isso `style-src` permite estilos inline; scripts inline, `unsafe-eval`, rede,
frames e objetos continuam bloqueados. Conteúdo de arquivos é sempre inserido
como texto no editor, nunca interpretado como HTML.

## Logging

Logs podem registrar versão, classe do evento, duração, código de erro e decisão
de política. Não devem registrar tokens, valores de secret store, conteúdo de
arquivos/conversas, environment completo ou comandos contendo dados redigidos.
E-mail, URLs de autenticação, códigos de dispositivo e IDs de login também não
são telemetria.

## Estado atual

O editor lista, busca, cria, lê, salva, aplica patches, move e exclui arquivos
por uma fronteira Rust limitada à raiz do projeto, com revisões e precondições de
conflito. Binários nunca são abertos como texto, symlinks são recusados e exclusões
confirmadas são movidas para recuperação privada durante a sessão. Eventos de
mutação identificam operação, origem local e revisões sem registrar conteúdo. O
IDE inicia sem provider de IA e não
consulta conta nem registra eventos de agente sem um provider ativo. O registro
resolve essa opção por tipo, capabilities e grants, sem identidade fixa; zero é
um estado normal e ambiguidade falha fechada. O adapter experimental Codex exige
HTTPS e hosts OpenAI permitidos para login; tokens não cruzam a fronteira Rust.
Processos, rede e filesystem solicitados por qualquer plugin permanecem sujeitos
às permissões e approvals do núcleo.

Manifests de plugin usam schema e enums fechados. Origem, compatibilidade e
entrypoint são validados antes do catálogo; concessões ficam separadas da
declaração. Uma mudança de permissões desabilita o plugin até nova revisão, e o
adapter Codex verifica declaração e concessão em cada entrada sensível.

O instalador local de `.tar.zst` compara o pacote com um descritor SHA-256
externo e extrai em staging privado com limites de tamanho e quantidade. Ele
recusa traversal, links, tipos especiais e duplicatas, valida novamente o
manifesto e publica a versão por rename atômico somente após revisão exata das
permissões. O pacote nasce desabilitado e não executável; download e execução só
ocorrem pelas fronteiras autenticadas do catálogo e do broker sandboxed.

A interface abre a seleção pelo núcleo Rust e não envia paths pelo IPC. Para a
revisão, recebe apenas dados tipados e um token opaco ligado ao único staging
pendente. Confirmar exige o mesmo token e exatamente as permissões do manifesto;
cancelar ou substituir a sessão remove os temporários. Campos não confiáveis do
manifesto são inseridos na interface como texto, sem interpretação HTML.

Cada instalação mantém um recibo host-managed com hash determinístico da árvore
extraída. O catálogo recalcula esse hash e revalida layout, identidade, versão,
manifesto e entrypoint em todo reload. Corrupção, links, arquivos especiais ou
tentativa de sobrepor um ID embutido removem todos os plugins externos do estado
de autoridade. Atualizações também removem habilitação e grants até nova
revisão. Para instalações do catálogo, o recibo também preserva a assinatura e o
ID da chave do publisher; instalações locais são identificadas como não autenticadas.

Ao remover um plugin externo, o núcleo valida o destino e move todas as versões
por rename atômico para uma quarentena fora do catálogo. Preferências e grants só
são publicados depois da reconstrução e persistência; falhas restauram o diretório.
Se o rollback falhar, a autoridade externa falha fechada. Resíduos de uma queda
depois do rename são apagados antes da descoberta na próxima inicialização.

O catálogo v2 embarcado é a base compilada de uma cadeia de confiança. Updates
remotos possuem versão monotônica, expiração, limiar de assinaturas raiz e chaves
Ed25519 delegadas e revogáveis de publishers. Assinaturas cobrem JSON canônico com
separação de domínio; IDs são derivados da chave pública e chaves fracas são
recusadas. Replay, rollback, congelamento e downgrade falham antes da persistência
atômica. A raiz só pode mudar com nova versão do aplicativo e updates permanecem
bloqueados enquanto nenhuma chave pública oficial estiver provisionada.

Cada assinatura de publisher cobre manifesto, descritor SHA-256 e tag. O frontend
solicita somente o ID; URL e destino são derivados no núcleo. Downloads usam HTTPS,
timeout, redirects limitados a hosts exatos do GitHub e tamanho limitado durante o
streaming. Arquivos parciais são privados e limpos após erro ou reinicialização.
Manifesto e descritor baixados precisam ser exatamente os assinados. A autenticação
é revalidada em reloads futuros; chave ausente ou revogada faz a autoridade externa
falhar fechada.

Runtimes externos agora só iniciam pelo broker Linux com Bubblewrap após revalidar o
recibo e a árvore instalada. A política exige `process_spawn`, monta o pacote somente
para leitura e expõe apenas o workspace ativo
no modo concedido. Rede, ambiente e HOME são negados por padrão; secret storage e
approvals continuam mediados pelo host. O entrypoint executável existe somente em
uma sessão privada, com `no_new_privs` e limites de recursos. Falha de sandbox mantém
o plugin desabilitado, e desativação, remoção, troca de workspace ou encerramento
terminam o processo.

O transporte funcional usa stdin/stdout somente como JSONL v1. O handshake exige
versão e conjunto exato de capabilities do manifesto. Frames, profundidade,
strings, coleções, eventos e tempo são limitados; request IDs são gerados pelo host
e respostas precisam repetir ID e capability. Operações pertencem ao namespace da
capability declarada. Mensagem fora de ordem ou inválida encerra o runtime. Payloads
não são encaminhados ao frontend nem interpretados como comandos ou autoridade.

Commands de conta, chat, ferramentas e approvals repetem a resolução do provider
e a verificação das capabilities e permissões necessárias antes de alcançar o
adapter. O frontend recebe somente um resumo tipado do provider ativo e não
seleciona autoridade pelo catálogo. Runtimes de IA sem adapter tipado são
recusados, mesmo que seu handshake externo seja válido.

O broker geral de processos agora possui revisão e execução separadas por token
opaco de uso único. Cwd permanece sob o workspace sem symlinks, argv não passa por
shell implícito e scripts de shell são exibidos exatamente. Modos somente leitura e
workspace-write exigem Bubblewrap funcional e falham fechados; host escalated exige
autoridade independente. Environment começa vazio, rede é negada por padrão, saída
é drenada com captura limitada e cancelamento/timeout encerram o grupo de processos.
Auditoria contém o hash do comando, nunca seu conteúdo. Tasks externas são catálogos
tipados e limitados; o núcleo consulta a definição novamente, deriva autoridade dos
grants persistidos e entrega à interface apenas uma revisão com token opaco. Grants
mudados e lifecycle do plugin/workspace invalidam revisões e cancelam processos.

Approvals do agente e revisões de Tasks agora também exigem o SHA-256 integral da ação
apresentada pelo frontend. Comando, cwd, arquivos, domínio, environment e política
fazem parte do vínculo conforme o tipo; qualquer divergência preserva ou nega o token
sem executar o efeito. Aprovações expiram em cinco minutos e decisões duplicadas não
encontram mais a solicitação consumida. Regras de sessão ficam no núcleo, limitadas à
conversa e ao hash exato, são revogáveis nas Configurações e nunca são delegadas como
política persistente ao provider. O histórico em memória contém somente categoria,
decisão, origem, horário e hash, sem comandos, diffs ou valores de environment.

O inspector Git usa diffs reais de worktree e índice. O commit local exige revisar
o diff completo de uma árvore imutável e confirmar um token de uso único, com
validade de cinco minutos. Mudanças no índice/HEAD/branch invalidam a revisão.
O commit publica somente a árvore revisada e não reescreve o índice. Paths são
literais, plugins de diff/textconv não executam e conteúdo é apresentado como texto.
A ADR-0018 documenta o contrato, os limites e a corrida residual de checkout.

## Recuperação do editor — 2026-09-25

Rascunhos agora persistem em arquivos privados 0600, sob diretório 0700 por perfil,
fora do workspace. Isso inclui eventual conteúdo sensível digitado pelo usuário;
não há criptografia nem proteção contra processos do mesmo usuário/root. O fluxo
não envia esse conteúdo a plugins, logs ou rede. Tamanho e quantidade são limitados.
Snapshot corrompido ou de versão desconhecida não é sobrescrito automaticamente.

Escritas usam temporário exclusivo, sincronização, rename e lock/CAS por workspace.
Os commands só aceitam o projeto atualmente aberto. Recuperar um rascunho preserva
sua revisão original e não escreve no workspace; disco divergente ou indisponível
exige salvar uma cópia nova ou descarte explícito antes de reler. O fechamento usa
`onCloseRequested` e concede `core:window:allow-destroy` ao helper do Tauri para
concluir depois do flush. Emissão de eventos continua negada. Limites e retenção
estão na [ADR-0020](../architecture/ADR-0020-editor-session-recovery.md).

## Diagnósticos Rust — 2026-09-25

LSP acrescenta um programa externo com leitura dos rascunhos e do projeto. Rust
0.1.1 exige revisão da nova permissão `process_spawn`; ela não é concedida na
migração nem nos defaults. Start/sync/status repetem a autorização e a identidade
do plugin. A comunicação possui versões e identificador de sessão; diagnósticos
fora de URI/versão são ignorados e conteúdo do servidor é mostrado como texto.
Não existe roteamento de comandos LSP ou aplicação de edits ao workspace.

O servidor roda exclusivamente via Bubblewrap, com projeto read-only, rede
isolada e ambiente/caches privados. Configurações desativam build scripts, proc
macros e checks; arquivos `rust-analyzer.toml` existentes no código são mascarados
no mount. Esse mascaramento não cria um snapshot imutável do projeto: a fronteira
contra execução descoberta por ferramentas é o sandbox, não uma promessa de que
opções de servidor validam código. URI/ranges, framing, filas e documentos têm
limites. Desativação, troca de projeto e fechamento encerram o processo. Falhas
limpam os diagnósticos e permitem reinício, mantendo o editor utilizável.

Escopo, limites e bibliotecas não carregadas estão na
[ADR-0021](../architecture/ADR-0021-rust-lsp-diagnostics.md).

### Consultas Rust de leitura

Definição e hover usam IPC tipado, sessão/versão/posição e autorização antes e
depois de esperar pelo servidor. Até oito consultas ficam pendentes por sessão;
o prazo é de cinco segundos e respostas obsoletas são descartadas. O worker
sincroniza os documentos antes de emitir a consulta. URIs de definição precisam
pertencer ao workspace e ranges devem caber no texto local. Leituras de validação
em arquivos não abertos percorrem descritores de diretórios com O_NOFOLLOW e
aceitam somente arquivos regulares limitados. Hover é texto inerte, inclusive
quando recebido como Markdown/HTML. O fluxo de abertura de abas mantém os riscos
residuais de alterações externas da fronteira de workspace existente; resultados
LSP não constituem snapshots do disco. Contrato na ADR-0022.

### Toolchains e fontes externas de Rust

A ADR-0023 permite escolher uma instalação existente e compartilhar somente os
subdiretórios registry/index, registry/cache e registry/src revisados, com opt-in
separado para cache. Tokens de revisão são vinculados ao workspace, de uso único
e expiram em dez minutos; não são aceitos paths arbitrários do frontend. O serviço
revalida a disponibilidade da seleção, limpa a autorização ao trocar projeto ou
revogar Rust e não monta HOME/configuração/credenciais do Cargo. Conteúdo de crates
privadas presentes nos diretórios concedidos também é legível pelo servidor.
Cargo opera offline/locked, com cache/workspace somente leitura. Fontes externas
só chegam ao visualizador após validação de URI, raiz concedida, arquivo regular,
limites e ranges. Não há comando de edição ou leitura externa genérica. Diretórios
autorizados permanecem mutáveis por processos do host; não há identidade por hash
de toolchain ou snapshot de cache, e a seleção pressupõe confiança na instalação.

### Ações de edição Rust

Completion/references/rename/formatting reutilizam a sessão LSP autorizada e suas
proteções de versão/cancelamento. Não existe passagem livre de métodos ou comandos.
Comandos de completion e operações de arquivos em WorkspaceEdit são recusados;
edições precisam de ranges UTF-16 válidos e sem sobreposição. Bibliotecas são
somente leitura. Rename captura fontes fechadas antes da consulta e compara textos;
o frontend prepara todos os arquivos antes de aplicar edições versionadas em memória.
Falhas de leitura, rascunhos divergentes ou recuperação pendente rejeitam o conjunto.
Rascunhos de abas inativas participam do backup. A gravação segue o CAS do editor.
Rustfmt vem da toolchain selecionada via RUSTFMT e herda o isolamento do servidor.
Os limites e a ausência de watcher de disco estão na ADR-0024.

### Atualizações externas do workspace

A observação periódica é limitada ao projeto ativo, roda no pool bloqueante e
não mantém threads/descriptors entre consultas. Travessia por diretórios fixados
em descritores impede redirecionamento por symlinks. Leitura automática usa raiz
validada, O_NOFOLLOW por componente, arquivo regular, O_NONBLOCK e limite de bytes.
Tokens de polling não concedem acesso a outros projetos. Respostas antigas não
atingem novas abas/workspaces; saves em andamento são adiados. Conteúdo alterado,
excluído, binário ou indisponível nunca substitui um rascunho divergente. Detectar
mudanças Rust invalida consultas antigas no backend antes de reiniciar a análise.
A observação tem limites e latência explícitos (ADR-0025); não substitui o CAS do
salvamento nem observa diretórios externos do cache/toolchain.

### Correções rápidas e resolução de imports

`code_action` retorna apenas quickfixes com WorkspaceEdit validado. O levantamento
prévio de fontes e a preparação integral/versionada de rascunhos são os mesmos da
renomeação; nenhum modelo é criado até a seleção da correção. Comandos, recursos
externos, operações de arquivos e edições anotadas/sobrepostas são descartados.
`completion_resolve` aceita somente UUIDs gerados pelo backend para itens recebidos
na sessão atual, vinculados a caminho, posição, versão e revisão, com expiração e
cache limitado. Dados brutos do servidor não são aceitos do frontend. Resolução
não pode alterar a inserção original; imports adicionais são validados e resolvidos
antes da aceitação da sugestão. Não há grant novo de escrita, comandos LSP genéricos
ou salvamento implícito. Limites e janela de detecção de mudanças externas seguem
as ADRs 0024–0025; detalhes na [ADR-0026](../architecture/ADR-0026-rust-quick-fixes-and-auto-imports.md).


### Monitor incremental Linux

A [ADR-0027](../architecture/ADR-0027-incremental-workspace-monitor.md) mantém
watches de diretórios, com limites de entradas, caminhos, profundidade e fila.
Nomes recebidos de eventos apenas selecionam caminhos relativos: as leituras
reabrem componentes sem seguir links a partir da raiz autorizada, em vez de ler
pelo inode observado. Pastas movidas para fora não autorizam fontes externas.
Mudança de projeto e destruição da janela liberam a instância inotify. Overflow
reconstrói o cache e força ressincronização; cota insuficiente libera watches
parciais e expõe o modo de compatibilidade. Lotes inválidos preservam o snapshot
anterior. Abas abertas mantêm hashes limitados; fontes fechadas passam a depender
de eventos/metadados e da auditoria periódica. Nenhuma gravação é adicionada.
