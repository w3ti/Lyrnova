# Threat model inicial do IDE e dos plugins

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
| Patch sobre arquivo alterado | precondition/hash e conflito explícito |
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
