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
| Command injection | argumentos estruturados; shell somente explícito |
| Plugin tenta ler segredo | roots, deny rules e redaction |
| Exfiltração | rede negada por padrão e approval por destino |
| Approval reaproveitada | vínculo ao hash da ação exibida |
| Patch sobre arquivo alterado | precondition/hash e conflito explícito |
| Processo continua após cancelamento | process group/job e cleanup |
| ANSI/Markdown injeta UI | sanitização e CSP |
| Prompt injection em arquivo | conteúdo não amplia authority |
| Plugin/backend comprometido | protocolo limitado e policy engine independente |
| Frontend injeta path de pacote | seleção nativa e path mantido somente no núcleo Rust |
| Confirmação troca permissões revisadas | token efêmero e igualdade exata no command Rust |
| Remoção promove versão antiga | quarentena atômica do diretório completo do ID |
| Falha após remoção física | rollback antes do commit; falha fechada se rollback falhar |
| Frontend troca URL ou hash de download | IPC aceita somente ID do catálogo embarcado |
| Redirect de release causa SSRF | HTTPS e allowlist exata verificados em cada salto |
| Download sem `Content-Length` esgota disco | limite aplicado durante o streaming |
| Catálogo tenta downgrade | versão comparada antes e depois da rede sob lock |
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

O editor já lê e salva arquivos UTF-8 existentes por uma fronteira Rust
limitada à raiz do projeto, com revisão de conflito. Ele ainda não cria nem
exclui arquivos e recusa symlinks. O IDE inicia sem provider de IA e não
consulta conta nem registra eventos de agente sem o plugin Codex ativo. A base
experimental do plugin exige HTTPS e hosts OpenAI permitidos para login;
tokens não cruzam a fronteira Rust. Processos, rede e filesystem solicitados
por qualquer plugin permanecem sujeitos às permissões e approvals do núcleo.

Manifests de plugin usam schema e enums fechados. Origem, compatibilidade e
entrypoint são validados antes do catálogo; concessões ficam separadas da
declaração. Uma mudança de permissões desabilita o plugin até nova revisão, e o
adapter Codex verifica declaração e concessão em cada entrada sensível.

O instalador local de `.tar.zst` compara o pacote com um descritor SHA-256
externo e extrai em staging privado com limites de tamanho e quantidade. Ele
recusa traversal, links, tipos especiais e duplicatas, valida novamente o
manifesto e publica a versão por rename atômico somente após revisão exata das
permissões. O pacote nasce desabilitado e não executável. Download e execução
de pacotes externos continuam desativados.

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
revisão. O recibo não substitui assinatura criptográfica do publisher.

Ao remover um plugin externo, o núcleo valida o destino e move todas as versões
por rename atômico para uma quarentena fora do catálogo. Preferências e grants só
são publicados depois da reconstrução e persistência; falhas restauram o diretório.
Se o rollback falhar, a autoridade externa falha fechada. Resíduos de uma queda
depois do rename são apagados antes da descoberta na próxima inicialização.

O catálogo curado é parte imutável do binário e associa cada manifesto a um
descritor SHA-256 e uma tag. O frontend solicita somente o ID; URL e destino são
derivados no núcleo. Downloads usam HTTPS, timeout, limite de redirects e hosts
exatos do GitHub, com limite de tamanho durante o streaming. Arquivos parciais são
privados e limpos após erro ou reinicialização. Hash e manifesto são revalidados
antes da revisão, e versões iguais ou inferiores à instalada são recusadas.
