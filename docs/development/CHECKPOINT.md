# Checkpoint de desenvolvimento

Data: 2026-09-02
Commit-base: `c0e16d2`
Estado: base do item #15 publicada; integração tipada de Tasks local.

## Direção consolidada

- Lyrnova é um IDE desktop extensível, não um cliente Codex.
- Linguagens, frameworks, runtimes e provedores de IA são plugins opcionais.
- A instalação inicial não inclui nem ativa o Codex.
- O projeto usa Rust + Tauri 2 e uma interface inspirada no fluxo familiar do VS Code/Codex.
- Nada deve ser publicado, enviado ao GitHub ou ao OBS sem autorização explícita.

## Último trabalho concluído localmente

- Contrato v1 de `plugin.json` implementado com schema JSON e validação Rust
  estrita.
- Catálogo hardcoded substituído por manifests embutidos e validados para Rust,
  Web Essentials e Codex.
- Capabilities, permissões, tipos, runtime e origem passaram a usar enums fechados.
- Estado de plugins migrado para v3 com concessões separadas; mudança de permissões
  desabilita o plugin até nova revisão.
- Instalação exige aprovação exata das permissões e operações sensíveis do adapter
  Codex verificam declaração e concessão.
- Manifests externos não podem se declarar embutidos; processos exigem entrypoint
  relativo, protocolo conhecido e `process_spawn`.
- Pacotes externos `.tar.zst` usam descritor SHA-256 separado do manifesto para
  evitar a circularidade de um arquivo declarar o próprio hash.
- Instalador local em duas fases implementado: staging/inspeção e instalação após
  aprovação exata das permissões.
- Extração limitada por tamanho comprimido, fluxo descomprimido, arquivo, soma e
  quantidade; traversal, paths não UTF-8, links, tipos especiais e duplicatas falham.
- Manifesto externo e entrypoint são revalidados no staging; falha ou abandono limpa
  temporários; instalação usa rename atômico, não substitui versão e nasce desabilitada
  e sem bit de execução.
- Instalações externas recebem recibo host-managed e SHA-256 determinístico da árvore,
  cobrindo paths, tipos, modos, tamanhos e conteúdo.
- O catálogo dinâmico redescobre e revalida pacotes a cada reload, escolhe a versão
  SemVer mais recente e impede que externos substituam IDs embutidos.
- Corrupção ou layout inválido falha fechado para todo o catálogo externo, removendo-o
  também do estado de autoridade em memória.
- Estado de plugins migrado para v4 com versões instaladas; upgrades removem grants e
  habilitação até nova revisão; aprovação externa pode ser persistida sem ativação.
- Alterações de instalação, remoção e habilitação só entram em memória depois de a
  persistência ser concluída.
- Configurações agora listam o catálogo real e permitem selecionar um `.tar.zst`
  por diálogo nativo, revisar manifesto, integridade e permissões e confirmar ou
  cancelar a instalação.
- O frontend não envia paths de pacote: recebe uma revisão tipada e um token opaco
  para o único staging pendente. O núcleo exige token válido e aprovação exata.
- Cancelamento e substituição limpam o staging; instalações confirmadas entram no
  catálogo, mas continuam desabilitadas até ativação explícita.
- O catálogo v2 valida versão, expiração, limiar Ed25519 da raiz e assinaturas
  delegadas de publishers sobre JSON canônico. Replay, rollback, downgrade,
  adulteração e chaves revogadas falham fechados antes da persistência atômica.
- O recibo distingue pacotes locais não autenticados de releases assinadas e a
  identidade do publisher é revalidada em todo reload. Updates remotos ficam
  bloqueados até a chave pública raiz oficial ser provisionada no aplicativo.
- Dados do manifesto são renderizados como texto na interface, sem HTML não confiável.
- A remoção externa move atomicamente o diretório completo do ID para uma quarentena,
  evitando que versões antigas sejam promovidas após a exclusão.
- Catálogo, habilitação e concessões são persistidos antes da limpeza; falha restaura
  o pacote, e falha de rollback remove toda autoridade externa da memória.
- Instalação e remoção usam o mesmo lock global, e resíduos de uma interrupção são
  limpos antes da descoberta na próxima inicialização.
- A interface oferece remoção somente para plugins externos e exige confirmação.
- Catálogo v2 estrito e embarcado adicionado como base compilada para downloads;
  ele começa vazio até uma release real ser revisada.
- O frontend solicita download apenas por ID. URL de GitHub Release, tag, descritor,
  hash e destino permanecem sob controle do núcleo Rust.
- Downloads HTTPS verificam allowlist em cada redirect, usam timeout, diretório
  privado e limite de 64 MiB durante o streaming; parciais são limpos no reinício.
- Versões iguais e downgrades são bloqueados antes da rede e novamente sob o lock;
  pacotes baixados passam pela mesma revisão e nascem desabilitados.
- Broker Linux de runtimes externos usa Bubblewrap obrigatório e falha fechado nas
  demais plataformas ou quando o sandbox está ausente.
- Policy engine materializa `workspace_read`, `workspace_write` e `network_access`
  como mounts/rede do sandbox; ambiente, HOME e secrets permanecem isolados.
- Entrypoints continuam não executáveis no pacote e recebem cópia privada somente
  durante a sessão, com limites de recursos e cleanup de lifecycle.
- Ativação inicia o runtime; desativação, remoção, troca de workspace e encerramento
  terminam o processo. Falha no reinício desabilita o plugin externo.
- Runtimes externos usam apenas JSONL v1 em stdin/stdout e precisam concluir um
  handshake com versão e capabilities exatamente iguais ao manifesto.
- Requests recebem IDs do host e operações namespaced pela capability; respostas,
  erros e eventos repetem a fronteira tipada. Frames, payloads, filas e tempo são
  limitados, e qualquer violação encerra a sessão.
- Providers de IA ativos são resolvidos por tipo, capabilities e grants, sem ID
  fixo no núcleo ou no frontend. Nenhum provider é um estado normal; múltiplos
  providers ativos falham fechados até existir uma escolha persistida.
- Commands de conta, chat, ferramentas e approvals repetem a autorização antes do
  adapter. Runtimes de IA sem adapter tipado são recusados, sem encaminhar payload
  genérico ao frontend.
- Manual para terceiros documenta manifesto, capabilities, permissões, entrypoint,
  sandbox, protocolo, empacotamento reproduzível, sidecar SHA-256, testes, updates
  e preparação para publicação autenticada.
- O manual explicita que a instalação local é a distribuição disponível hoje e que
  o catálogo curado aguarda chaves raiz e tooling oficial de assinatura.
- Licença do código autoral migrada de MIT para `GPL-3.0-only`.
- Texto integral oficial da GNU GPL versão 3 instalado em `LICENSE`.
- Metadados Cargo, npm, AppStream e RPM atualizados, sem alterar as licenças das
  dependências.
- Documentação de escopo e README atualizados para refletir a nova licença.
- Área de configurações implementada.
- Configurações funcionais para fonte do editor, família tipográfica, tamanho das tabs,
  quebra de linha, espaços em branco, minimapa, ligaturas, rolagem suave, fonte do
  terminal e confirmação ao fechar arquivo modificado.
- Aparência do aplicativo oferece tema do sistema, escuro, claro e alto contraste,
  tamanho independente da fonte da interface, densidade compacta/confortável e
  redução de movimento; o Monaco acompanha a paleta escolhida.
- Configurações são persistidas localmente e aplicadas ao Monaco em tempo real.
- Atalho `Ctrl+,`, entrada na paleta de comandos e botão de engrenagem adicionados.
- Sugestões futuras exibidas para zoom por workspace, atalhos, salvamento/formatação,
  perfis do terminal, Git e privacidade dos plugins.
- Tela vazia do editor mostra o ícone do Lyrnova quando todas as abas são fechadas.
- `WorkspaceService` agora oferece metadados, leitura textual por faixa, busca limitada por nome/conteúdo,
  criação exclusiva, movimentação sem substituição e patch textual com revisão,
  ranges UTF-8, precondições exatas e preview sem escrita.
- Paths relativos ambíguos ou não portáveis são recusados antes do efeito; entradas
  existentes são canonicalizadas sob a raiz e symlinks são rejeitados.
- Leitura, criação e salvamento textual recusam NUL/binários; arquivos grandes e
  binários podem continuar sendo listados e administrados sem chegar ao editor.
- Exclusão confirmada move a entrada para recuperação privada por token opaco e o
  Explorer mantém uma pilha de ações que podem ser desfeitas durante a sessão.
- Criação, mover/renomear, excluir, restaurar, atualizar e busca nativa foram ligados
  ao Explorer; rascunhos sujos bloqueiam mutações que os afetariam.
- Toda mutação emite evento atribuído a `local_user`, com UUID, operação, paths e
  revisões quando aplicáveis. A ADR-0015 documenta contratos e risco TOCTOU residual.
- O item #15 começou com um broker de processos em duas fases: revisão por token
  opaco de uso único e execução do plano imutável, com autoridade independente para
  escrita, rede e modo escalated.
- Argv estruturado não passa por shell; scripts de shell são explícitos e exibidos
  exatamente. Cwd e executáveis locais ficam sob a raiz, symlinks são recusados e o
  environment parte vazio com allowlist curta.
- Read-only e workspace-write exigem Bubblewrap funcional e falham fechados. O
  sandbox nega rede por padrão, monta somente o necessário e possui diagnóstico
  real; host escalated nunca é fallback automático.
- Stdout/stderr são drenados concorrentemente com captura de 1 MiB por stream;
  timeout e cancelamento encerram filhos/netos pelo grupo. Core, arquivos, memória,
  descritores e forks têm limites, e a auditoria registra apenas SHA-256 do comando.
- Plugins externos agora oferecem catálogo `tasks.list` estrito e limitado. O
  frontend seleciona somente plugin + ID; o Rust consulta novamente a definição e
  deriva toda autoridade dos grants persistidos.
- O `TaskBroker` cria revisão por token, revalida o conjunto exato de grants antes
  da execução e nunca permite modo escalated para plugins. Mudança de plugin ou
  workspace invalida revisões e cancela processos associados.
- A interface ganhou área de Tasks, diagnóstico de sandbox, diálogo com comando,
  cwd, rede, acesso e risco, streaming limitado no dock e cancelamento explícito.
- A ADR-0016 registra a integração concluída e os riscos residuais de cgroup/TOCTOU.

## Validações já executadas em 2026-09-02

- `npm run check --prefix ui`
- `npm run build --prefix ui`
- `cargo fmt --all -- --check`
- `cargo check --workspace --offline`
- `cargo clippy --workspace --all-targets --offline -- -D warnings`
- `cargo test --workspace` (143 testes aprovados e 1 teste de integração
  opcional ignorado por exigir Codex App Server local)
- `cargo metadata --offline --no-deps --format-version 1`
- `npm install --package-lock-only --ignore-scripts --offline --prefix ui`
- `appstreamcli validate --no-net`
- `xmllint --noout`
- `rpmspec -P`
- `git diff --check`

O smoke test de runtime iniciou e encerrou um processo real no Bubblewrap. O
contêiner de desenvolvimento permite os namespaces principais com rede concedida,
mas bloqueia a criação do namespace de rede isolado; a negação de rede foi validada
pela construção determinística da política e permanece fail-closed no lançamento.

Todas passaram. Em uma validação visual anterior, a área base de configurações foi
acionada no WebView e também renderizada a partir do bundle local para inspeção em
1600 × 1000. Hierarquia, espaçamento, contraste, controles e convivência com o painel
do terminal foram revisados sem defeitos visuais bloqueantes.

## Ponto exato da licença

O código autoral usa a expressão SPDX `GPL-3.0-only`. O `LICENSE` tem 674 linhas e
corresponde byte a byte ao texto oficial da GNU GPL versão 3 instalado no sistema
(SHA-256 `3972dc9744f6499f0f9b2dbf76696f2ae7ad8af9b23dde66d6af86c9dfb36986`).
Referências a MIT, MPL, Apache, ISC e BSD que permanecem no lockfile pertencem a
dependências e não devem ser substituídas.

## Ao retomar

1. preservar a decisão `GPL-3.0-only` em novos metadados e templates;
2. continuar sem commit, push, publicação ou envio ao OBS sem autorização explícita;
3. próximo item proposto: issue #16, “Criar fluxo claro de permissões e approvals
   para plugins e tarefas”.

## Estado do repositório

O commit `c0e16d2` contém a base segura do broker de processos. A integração do item
#15 com Tasks, grants e revisão na interface está no worktree, ainda sem commit.
Nenhum pacote OBS ou release foi realizado.
