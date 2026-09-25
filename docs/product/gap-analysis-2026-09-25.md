# Levantamento de pendências do produto — 2026-09-25

Atualização após o levantamento: a primeira entrega foi implementada localmente,
com diff real de worktree/índice, revisão integral e commit vinculado ao snapshot.
Veja a [ADR-0018](../architecture/ADR-0018-git-diff-and-commit-review.md).
Também foi implementada e executada uma jornada no aplicativo real sem IA,
com correções de streaming e limpeza de processos do terminal. O gate automatizado
Linux passou; consulte a [evidência e os aceites pendentes](mvp-validation-2026-09-25.md).
Na sequência, o terminal passou a usar PTY Linux e xterm.js, com interação,
ANSI, resize e limpeza dos grupos de jobs. Escopo e limites na
[ADR-0019](../architecture/ADR-0019-linux-pty-terminal.md).
A quarta entrega implementa recuperação local de abas, cursor, rolagem e rascunhos,
com conflitos preservados e falhas de recuperação visíveis; veja a
[ADR-0020](../architecture/ADR-0020-editor-session-recovery.md). Serviço de secrets,
migrações futuras e homologação manual continuam pendentes. A quinta entrega traz
diagnósticos LSP iniciais de Rust, com revisão de permissões, servidor isolado,
marcadores e painel Problemas; veja a
[ADR-0021](../architecture/ADR-0021-rust-lsp-diagnostics.md). Passaram 186 testes
Rust e 20 grupos nativos, incluindo rust-analyzer real e recuperação de falha.
Definição, hover, integração de toolchains/dependências e demais recursos de
linguagem continuam pendentes; esta etapa não conclui o plugin Rust oficial.
O restante deste documento preserva o diagnóstico anterior à implementação;
diff multi-escopo avançado e validação global do MVP continuam pendentes.

O Lyrnova tem uma fundação funcional de IDE e uma implementação substancial das
fronteiras de segurança e plugins. Ainda faltam fluxos completos de desenvolvimento
para qualificá-lo como MVP validado ou alpha de linguagens: diff real, terminal PTY,
restauração de documentos, integrações LSP/DAP e plugins oficiais utilizáveis.

Base examinada: commit `c338103712fc67578434498b81eef5626f158fef`, também confirmado
como HEAD remoto de `main` nesta consulta. Foram cruzados código, documentos,
workflow de qualidade e as 43 issues do GitHub. Todas estavam abertas; isso não
equivale a 43 implementações ausentes. A classificação abaixo é uma avaliação do
código, não uma homologação nem uma alteração no estado das issues.

**O que já existe**

- Criação e abertura nativa de projetos, histórico do último projeto e operação sem provider de IA.
- Monaco, abas, edição e salvamento com revisão, proteção de rascunhos, Explorer e busca limitada por nome/conteúdo.
- Operações de criação, movimentação, patch e exclusão recuperável de arquivos.
- Git status, stage, unstage e commit local reais.
- Configurações de editor/aparência, alto contraste, movimento reduzido e dimensões de painéis persistidas.
- Manifestos e protocolo de plugins, instalação/remoção transacional, grants e runtimes externos isolados no Linux.
- Broker de processos e Tasks, revisão antes de executar, streaming, timeout e cancelamento.
- Approvals vinculados ao hash da ação, expiração, regras de sessão revogáveis e histórico em memória.
- Receita RPM/openSUSE, preparador de fontes OBS e CI de Rust/frontend.

**Pendências priorizadas**

As prioridades abaixo são propostas para execução, não uma reprodução das labels
do GitHub. P0 fecha lacunas de confiança e validação do produto atual; P1 entrega
o uso diário e a extensibilidade; P2 prepara expansão e distribuição. Correções
graves descobertas na validação devem preceder qualquer avanço de marco.

| Prioridade | Entrega | Situação comprovada e trabalho restante | Critério de conclusão proposto | Issues |
|---|---|---|---|---|
| P0 | Diff real e revisão antes de commit | Status/stage/commit funcionam, mas o inspector contém arquivos, contagens e diff fixos de demonstração. Implementar diff unstaged/staged/base, estados vazio/erro, renames/binários/conflitos e invalidação após mudança externa. | Um repositório temporário mostra exatamente as mudanças reais; revisar staged diff antes de confirmar commit; nenhum dado fictício no app nativo. | [#20](https://github.com/w3ti/Lyrnova/issues/20), [#22](https://github.com/w3ti/Lyrnova/issues/22) |
| P0 | Validar MVP ponta a ponta | CI atual verifica build frontend, sintaxe, fmt, clippy e testes Rust. Falta suíte de jornadas da UI nativa e evidência de uso sem providers, falhas de plugins e reinício. | Abrir/criar projeto, editar/salvar, buscar, revisar/commitar, executar/cancelar e reiniciar em ambiente limpo; registrar tempos/memória e decisão de aprovação do marco. | [#17](https://github.com/w3ti/Lyrnova/issues/17), [#8](https://github.com/w3ti/Lyrnova/issues/8) |
| P0 | Reconciliar documentação e backlog | Todas as issues estão abertas. O checkpoint ainda chama #16 de alteração local sem commit, mas ela está no HEAD remoto. O design system afirma que o terminal não executa comandos, contrariando o código. | Confrontar cada aceite com evidência, atualizar checkpoint e escopo real; separar implementado, validado e pendente. | #1–#16, [#34](https://github.com/w3ti/Lyrnova/issues/34) |
| P1 | Terminal PTY | Hoje há `/bin/bash` com stdin/stdout/stderr por pipes e entrada de uma linha; não há PTY nem emulador de terminal. | Shell interativo, ANSI, resize, Ctrl+C/EOF, limpeza da árvore de processos, scrollback limitado e sessões por projeto. | [#18](https://github.com/w3ti/Lyrnova/issues/18) |
| P1 | Persistência e recuperação | O último projeto, plugins, preferências e larguras persistem. A inicialização abre o primeiro arquivo da árvore, sem restaurar abas/cursor/rascunhos. Falta serviço de secrets do host. | Recuperar sessão por projeto após reinício/crash, preservar rascunhos, tratar corrupção/migração, oferecer recuperação e não reexecutar comandos. | [#6](https://github.com/w3ti/Lyrnova/issues/6), [#25](https://github.com/w3ti/Lyrnova/issues/25) |
| P1 | APIs de linguagem, debug e testes | LSP/DAP/testes/templates existem como capabilities declaradas; não foram encontrados consumidores funcionais equivalentes aos de Tasks. Monaco fornece suporte próprio e snippets Rust são registrados diretamente no frontend. | Ponte LSP com diagnósticos, definição, referências, rename e formatação; DAP com breakpoints/step/variáveis/stack; descoberta/execução de testes; lifecycle efetivo por plugin. | [#5](https://github.com/w3ti/Lyrnova/issues/5), [#19](https://github.com/w3ti/Lyrnova/issues/19), #39–#43 |
| P1 | Templates e configuração de projeto | Criar projeto gera README e .gitignore, com Git opcional. Tasks têm broker, mas faltam templates funcionais, detecção de toolchains e configuração versionada de ambientes/setup/instruções. | Criar um projeto compilável por template, mostrar ações previstas e executar build/test sem configuração manual interna ao IDE. | [#24](https://github.com/w3ti/Lyrnova/issues/24), #39–#43 |
| P1 | Plugins utilizáveis de ponta a ponta | Instalação local externa existe. Catálogo remoto tem zero entradas e raiz sem chaves. Faltam releases revisadas, tooling de assinatura, busca/filtros e dependências entre plugins. Há um caminho de instalação builtin no Rust sem chamada correspondente na UI atual. | Instalar um plugin útil em perfil limpo, revisar grants, ativar, usar, atualizar e remover; validar offline, corrupção e revogação. Publicação é uma etapa posterior, separada. | [#2](https://github.com/w3ti/Lyrnova/issues/2), [#10](https://github.com/w3ti/Lyrnova/issues/10), [#13](https://github.com/w3ti/Lyrnova/issues/13) |
| P1 | Primeira linguagem oficial completa | Rust e Web têm manifestos embutidos, mas isso não entrega o escopo dos plugins oficiais. Angular, React, Node.js e C/C++ não têm implementação própria neste checkout. | Começar por Rust: abrir/criar Cargo, LSP, build/test, erros navegáveis e debug; depois repetir o fluxo para Web/Node e frameworks. | [#39](https://github.com/w3ti/Lyrnova/issues/39), [#40](https://github.com/w3ti/Lyrnova/issues/40), [#41](https://github.com/w3ti/Lyrnova/issues/41), [#42](https://github.com/w3ti/Lyrnova/issues/42), [#43](https://github.com/w3ti/Lyrnova/issues/43) |
| P2 | Git profissional, review e worktrees | Falta gerenciamento de branches/remotes/worktrees, revisão inline e operações avançadas. O commit atual desabilita hooks e assinatura. | Diff e confirmação por operação; política explícita para hooks/assinatura; reverter recuperável, push com destino visível e isolamento de tarefas por worktree. | [#21](https://github.com/w3ti/Lyrnova/issues/21), [#22](https://github.com/w3ti/Lyrnova/issues/22), [#23](https://github.com/w3ti/Lyrnova/issues/23) |
| P2 | Providers opcionais completos | Existe adapter Codex experimental. Provider externo de processo retorna `ProviderUnsupported`; múltiplos ativos não têm escolha persistida; não há Gemini. O caminho de instalação Codex também precisa ser exposto pela UI. | Contrato tipado para providers, seleção persistida, secrets, contexto explícito, modelos, cancelamento/retomada e lifecycle completos; editor continua independente. | [#11](https://github.com/w3ti/Lyrnova/issues/11), [#37](https://github.com/w3ti/Lyrnova/issues/37), [#38](https://github.com/w3ti/Lyrnova/issues/38) |
| P2 | Distribuição e qualificação para release | Há receita openSUSE e CI Ubuntu; faltam matriz de pacotes, port Windows, SBOM, assinatura/proveniência, testes instalados e pipeline de release. | Build reproduzível a partir de tag limpa, instalação/upgrade/remoção testados por plataforma, artefatos verificáveis e promoção controlada. | [#26](https://github.com/w3ti/Lyrnova/issues/26)–[#30](https://github.com/w3ti/Lyrnova/issues/30), [#36](https://github.com/w3ti/Lyrnova/issues/36) |

**Condições para estabilidade**

- Segurança: fechar as janelas de troca concorrente de paths documentadas na
  ADR-0015/0016 com resolução por handles e complementar os limites de processos
  com cgroups. Fazer testes adversariais, revisão independente, política de reporte
  em SECURITY.md e registrar tratamento dos riscos residuais (#4, #14, #15, #31).
- Acessibilidade e internacionalização: já existem foco, live regions, contraste
  e movimento reduzido; falta homologar jornadas por teclado/leitor de tela,
  escalas e janelas pequenas, além de extrair strings para catálogos (#9, #32).
- Confiabilidade: medir projetos grandes, crash/restart de plugins, consumo,
  compatibilidade de toolchains e comportamento em Wayland/X11; versionar limites
  e bloquear regressões (#17, #33).
- Documentação e governança: guias de uso/instalação/privacidade/troubleshooting,
  política do SDK, proveniência dos assets e procedimento de publicação, denúncia,
  remoção e revogação de plugins (#7, #34, #35).
- Configurações: concluir atalhos editáveis, auto save/format on save, perfis do
  terminal, opções Git e preferências por projeto, hoje listadas como planejadas
  na própria interface (#25).

**Triagem das 43 issues**

“Substancial” indica implementação relevante encontrada, com aceites ainda a
conferir. “Parcial” indica partes entregues e lacunas concretas. “Pendente” indica
ausência da entrega principal ou de evidência de sua qualificação neste repositório.
Nenhuma linha recomenda fechar uma issue automaticamente.

| Issue | Avaliação | Trabalho que ainda exige atenção |
|---|---|---|
| #1 | Substancial | Atualizar estado real e comprovar limites/aceites dos marcos. |
| #2 | Substancial | SDK, dependências entre plugins e contratos funcionais restantes. |
| #3 | Substancial | Smoke nativo em perfil limpo e aceites completos. |
| #4 | Substancial | Riscos residuais, Windows e validação adversarial final. |
| #5 | Parcial | Consumidores/fixtures LSP, DAP, templates, testes e providers externos. |
| #6 | Parcial | Sessão/documentos, retenção, migração/recovery e cofre de credenciais. |
| #7 | Parcial | Assets existem; falta pipeline de derivados/checksums e validação do splash. |
| #8 | Substancial | Jornadas frontend, matrizes, dependências/licenças e evidência dos gates. |
| #9 | Substancial | Busca/filtros de plugins e matriz de layout/acessibilidade. |
| #10 | Substancial | Dependências, crash loop e cenários completos de recuperação. |
| #11 | Parcial | Autenticação genérica e secret store namespaced/mockável. |
| #12 | Substancial | Gestão completa de recentes/roots e aceites de nested repo/paths removidos. |
| #13 | Substancial | Catálogo operável, busca/filtros, instalação builtin e estados completos. |
| #14 | Substancial | Resolução de paths resistente a corridas e aceites de atribuição/cancelamento. |
| #15 | Substancial | Quotas por execução, fechamento de corridas de cwd e homologação no host alvo. |
| #16 | Substancial | Validar jornadas acessíveis e reconciliar checkpoint com commit publicado. |
| #17 | Pendente | Teste ponta a ponta, métricas e decisão de aprovação do MVP. |
| #18 | Parcial | Substituir pipes por PTY, interação, resize e ciclo completo de processos. |
| #19 | Substancial | Mudanças externas, escala, integração de linguagem e contexto extensível. |
| #20 | Parcial | Status real existe; diff real/multi-escopo ainda falta. |
| #21 | Pendente | Revisão e comentários inline vinculados a uma base imutável. |
| #22 | Parcial | Staged diff antes do commit, revert/push, política de hooks e auditoria. |
| #23 | Pendente | Gerenciamento e recuperação de worktrees. |
| #24 | Parcial | Tasks existem; configuração, templates, setup e hierarquia de instruções faltam. |
| #25 | Parcial | Restauração completa, safe mode, notificações e preferências restantes. |
| #26 | Parcial | Preparador existe; garantir checkout limpo, exclusões e reprodutibilidade. |
| #27 | Parcial | Receita existe; comprovar build e ciclo de pacote em ambientes limpos. |
| #28 | Pendente | Pacotes e matriz Fedora/Debian/Ubuntu/OpenBase. |
| #29 | Pendente | Port Windows, ConPTY, sandbox e validação de paths/instalador. |
| #30 | Parcial | CI de qualidade existe; faltam artefatos, matriz, SBOM e assinatura. |
| #31 | Pendente | Auditoria de segurança e política pública de reporte. |
| #32 | Parcial | Fundamentos de acessibilidade existem; faltam homologação e i18n. |
| #33 | Pendente | Baselines de desempenho/confiabilidade/compatibilidade. |
| #34 | Parcial | Arquitetura e manual de plugins existem; faltam guias finais e atualização. |
| #35 | Parcial | Licença/metadados existem; faltam governança e revisão de proveniência final. |
| #36 | Pendente | Release reconstruível, assinada, auditável e com runbook. |
| #37 | Parcial | Adapter existente; instalação acessível, seleção, contexto e lifecycle completos. |
| #38 | Pendente | Plugin Gemini. |
| #39 | Parcial | Manifesto/snippets Rust; faltam LSP/Cargo/templates/testes/debug integrados. |
| #40 | Pendente | Plugin Angular e suas dependências. |
| #41 | Pendente | Plugin React e suas dependências. |
| #42 | Pendente | Plugin Node.js e suas dependências. |
| #43 | Pendente | Plugin C/C++, toolchains/build/testes/debug. |

**Sequência de entrega proposta**

1. Reconciliar #1–#16 e corrigir o diff demonstrativo; instalar uma suíte de
   jornadas sem IA (#17), usada como gate contínuo.
2. Entregar PTY e recuperação da sessão; confirmar o ciclo diário
   abrir → editar → executar → revisar → commitar → reiniciar.
3. Implementar APIs LSP/templates/testes/DAP e uma primeira integração completa
   com Rust. Uma linguagem exercita a plataforma antes de multiplicar adapters.
4. Completar distribuição/lifecycle dos plugins, depois Web/Node e frameworks;
   desenvolver providers de IA como trilha opcional.
5. Acrescentar review/worktrees e concluir qualificação Linux, segurança,
   acessibilidade e empacotamento; portar e homologar Windows separadamente.

LSP, DAP, templates, testes, secret store, escolha de provider e dependências entre
plugins merecem entregas próprias com aceites. Hoje aparecem dispersos como
dependências em issues amplas, o que dificulta identificar o caminho crítico.

**Evidências e limites da avaliação**

| Constatação | Fonte |
|---|---|
| Escopo e marcos oficiais | [scope.md](scope.md) |
| Diff fixo de demonstração | [ui/index.html](../../ui/index.html), inspector `data-panel="changes"` |
| Operações Git disponíveis | [git.rs](../../src-tauri/src/git.rs), `GitService` |
| Bash por pipes | [terminal.rs](../../src-tauri/src/terminal.rs), `TerminalService::start` |
| Snippets Rust e restauro incompleto | [app.js](../../ui/app.js), `registerRustCompletions` e `initializeWorkspace` |
| Criar projeto sem templates | [lib.rs](../../src-tauri/src/lib.rs), `project_create_dialog` |
| Instalação builtin sem ação na UI | [app.js](../../ui/app.js), `renderPluginCatalog`; [lib.rs](../../src-tauri/src/lib.rs), `plugin_install` |
| Catálogo vazio e confiança não provisionada | [v2.json](../../plugins/catalog/v2.json), [catalog-root-v1.json](../../plugins/trust/catalog-root-v1.json) |
| Manifesto sem dependências e capabilities declaradas | [plugin_manifest.rs](../../src-tauri/src/plugin_manifest.rs) |
| Limite dos adapters de IA | [ai_provider.rs](../../src-tauri/src/ai_provider.rs), `adapter_for_runtime`; [ADR-0014](../architecture/ADR-0014-optional-ai-provider-resolution.md) |
| Riscos residuais de filesystem/processos | [ADR-0015](../architecture/ADR-0015-safe-workspace-operations.md), [ADR-0016](../architecture/ADR-0016-process-broker.md) |
| Cobertura declarada da CI | [quality.yml](../../.github/workflows/quality.yml) |
| Estado histórico da documentação | [CHECKPOINT.md](../development/CHECKPOINT.md), [design-system.md](../design-system.md) |
| Backlog remoto | [Issues](https://github.com/w3ti/Lyrnova/issues) |

A [execução Quality do HEAD](https://github.com/w3ti/Lyrnova/actions/runs/33680151581)
foi consultada e consta como bem-sucedida. Nesta análise executei
`npm run check --prefix ui`, que passou. Não refiz build/testes Rust, teste visual
ou instalação de pacotes; sucesso da CI não comprova os fluxos ausentes descritos
acima. Ausência de artefatos/testes locais foi tratada como falta de evidência,
sem presumir que nenhum teste manual externo tenha ocorrido.
