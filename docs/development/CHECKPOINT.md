# Checkpoint de desenvolvimento

## Retomada — 2026-10-02

A investigação pendente da pausa foi concluída; supera a "próxima ação" abaixo.

- Causa da falha em 200%: `clipboard_peer.py` fixava só `Gtk 3.0`; com o typelib
  do GTK 4 presente, `Gdk` carregava 4.0 e a importação falhava. A mensagem
  `A connection to the bus can't be made` no log não era a causa. O peer agora
  fixa `Gdk 3.0`, e o log dele vai para `clipboardPeerLog` no relatório.
- A verificação de overflow dava falso positivo de 1 px com o DPR fracionário do
  Xvfb (1,042/2,083; layout de 1382,4 CSS px). Agora compara com a largura
  fracionária do layout e lista elementos excedentes.
- Suíte de entrada com o harness final: 11 grupos em 100% e em 200% passaram
  (`target/e2e-desktop*/report.json`). Matriz atualizada em
  [desktop-validation](../product/desktop-validation-2026-09-27.md).
- Regressão completa local: 33 grupos com rust-analyzer real (reinstalado da
  release fixada no workflow, SHA-256 conferido).
- Entrega 13 publicada em `4f4aefc`. O CI expôs dois problemas independentes:
  clippy estável novo nega `AtomicUsize::fetch_update` (deprecated; `try_update`
  excede o MSRV 1.85), corrigido com laço `compare_exchange_weak` em `a7d59f1`;
  e consulta de referências Rust vazia durante reanálise, agora aguardada em
  `3d68703`. [CI de `3d68703`](https://github.com/w3ti/Lyrnova/actions/runs/37040056451)
  aprovado com as duas jornadas.
- Próximo: adicionar o gate de 200% ao workflow; depois reconciliar o backlog
  (triagem de 25/09 anterior às entregas 1–13).

## Ponto de retomada — pausa solicitada em 2026-09-27

Usuário pediu para guardar a posição e continuar depois. Trabalho interrompido
na entrega 13: acentuação, clipboard, foco e validação de escala.

- Último commit remoto: `adc0b4c90873faf729438fe0e02d9c61d76b6349`, `main`,
  [CI aprovado](https://github.com/w3ti/Lyrnova/actions/runs/36316630535).
- Alterações da entrega 13 estão **locais, sem commit/push**. Preservar a mudança
  preexistente de uma linha em branco em `docs/architecture/ADR-0001-rust-tauri.md`;
  ela não pertence a esta entrega.
- Correções de produção: paleta em `<dialog>` com Tab/Shift+Tab contidos e foco
  restaurado; comandos fecham a paleta antes de focar destino; atalhos globais
  respeitam outros modais/composição; fechar terminal focado retorna ao editor;
  Ctrl+Shift+C usa o comando de cópia do WebKit e a seleção do xterm.
- Passaram: sintaxe, 36 testes JS, builds frontend/Rust e regressão nativa completa
  com 33 grupos (`target/e2e-dialog-regression/report.json`). Nenhum Rust de
  produção foi alterado nesta entrega.
- A suíte de entrada passou em 100%, com 11 grupos, no relatório
  `target/e2e-desktop/report.json`, iniciado em `2026-09-27T11:51:55Z`.
  Isso inclui acentos, clipboard GTK ↔ Monaco, clipboard do terminal nos dois
  sentidos, Ctrl+C/D, foco, projetos e revisão de plugins.
- **O harness foi alterado depois desse sucesso e precisa ser revalidado.**
  Foram acrescentados `--scale 1/2`, métricas de apresentação e observação do
  clipboard pelo peer GTK, para aguardar processamento antes de trocar foco.
- Em 200%, uma primeira execução perdeu o final da digitação ao mudar o foco
  cedo demais. Após acrescentar sincronização/observação, a execução mais recente
  falhou antes da cópia: `Timed out: external GTK clipboard peer`, após quatro
  grupos. Evidência: `target/e2e-desktop-2x/report.json`, início
  `2026-09-27T11:54:30Z`; log `/tmp/lyrnova-desktop-input-2x.log`.
  Não considerar 200% aprovado.
- Próxima ação: diagnosticar a inicialização de `tests/e2e/clipboard_peer.py`.
  Ele passou a receber três caminhos (texto inicial, buffer observado, clipboard
  observado) e importar Gdk/Gtk. Capturar seu stderr antes de remover o diretório
  temporário; atualmente `clipboard-peer.log` fica na fixture temporária e não é
  preservado. Verificar também a nova callback de `owner-change`/`request_text`.
  Depois repetir a suíte em 100% e 200%, revisar evidência visual e atualizar
  `docs/product/desktop-validation-2026-09-27.md`.
- CI recebeu dependências `python3-gi` e `gir1.2-gtk-3.0`. O peer usa
  `/usr/bin/python3`; o workflow ainda executa somente a escala padrão. Adicionar
  o gate 200% apenas depois de validá-lo. Nenhum CI novo foi executado.
- Wayland nativo, escala fracionária 150%, IBus/Fcitx com candidatos, leitor de
  tela e arrastar/soltar continuam pendentes. Não declarar homologação global.

Comando local para retomar (usar tela gráfica isolada, com permissão apropriada):

```bash
python3 tests/e2e/native_input.py \
  --xvfb /tmp/lyrnova-xvfb/usr/bin/Xvfb \
  --driver /tmp/lyrnova-e2e-tools/bin/tauri-driver \
  --native-driver /tmp/lyrnova-e2e-tools/bin/WebKitWebDriver \
  --scale 2 --output target/e2e-desktop-2x
```

Para 100%, usar `--scale 1 --output target/e2e-desktop`. O binário em
`target/debug/lyrnova` já contém as correções de produção. Se alterar UI novamente,
executar `npm run build --prefix ui` antes de `cargo build -p lyrnova --bin lyrnova
--example e2e_fixture --offline --locked`. O wrapper local
`/tmp/lyrnova-dialog-regression.py` executa a regressão completa isolada, com
rust-analyzer em `/tmp/lyrnova-lsp-tools` e as ferramentas WebDriver acima.
Arquivos `/tmp` podem não sobreviver a reinício. Todos os testes desta rodada
encerraram; não ficou teste em andamento na pausa.

## Entrada, clipboard e foco — 2026-09-27

As entregas 11–12 foram publicadas em `adc0b4c` e passaram no
[CI remoto](https://github.com/w3ti/Lyrnova/actions/runs/36316630535), incluindo
as duas jornadas. Isso supera as indicações históricas de publicação pendente.

Entrega 13 local: teclas mortas e cedilha, clipboard Unicode com um editor GTK
externo, cópia/colagem do terminal e navegação de foco ampliam a suíte para 11
grupos. Foram corrigidos escape de Tab da paleta, restauração de foco e
Ctrl+Shift+C no terminal. Paleta usa `<dialog>`; atalhos globais respeitam os
outros modais. Regressão completa com 33 grupos e 36 testes JavaScript passou.
Detalhes, evidência e aceites restantes na
[matriz de desktop](../product/desktop-validation-2026-09-27.md).
Esta entrega ainda não foi publicada; Wayland, IME completo e leitor de tela
continuam exigindo homologação própria.

## Publicação das entregas 11–12 — 2026-09-27

Este conjunto reúne o monitor incremental e as correções de entrada e diálogos.
Validação local concluída: 218 testes Rust, 36 JavaScript, 33 grupos da jornada
nativa e seis grupos de entrada do SO; fmt/clippy/sintaxe/build aprovados.
A validação remota usa as duas jornadas no
[workflow Quality](https://github.com/w3ti/Lyrnova/actions/workflows/quality.yml).
As seções abaixo registram os resultados e o estado no momento de cada entrega.

## Entrada do SO e diálogos — 2026-09-27

Entrega 12, local: `tests/e2e/native_input.py` exercita teclado/mouse via XTest
num Xvfb privado, com perfil e D-Bus descartáveis. WebDriver apenas observa o DOM.
Passaram seis grupos: abertura/cancelamento, criação com Git, edição e atalhos,
terminal, troca de projeto com rascunho e instalação local com revisão de permissões.
Evidência: `target/e2e-input/report.json`. A suíte foi adicionada ao workflow,
mas esta entrega e o monitor incremental ainda não foram publicados.

Foram corrigidos bloqueio dos três seletores na thread da UI, modal de criação
que permanecia aberto, desfazer que atravessava o ponto salvo e conflito entre
Ctrl+K e o Monaco. Ctrl+K abre a paleta também no editor; F1 → **Show or Focus
Hover** acessa o hover por teclado. Ctrl+Shift+K continua sendo o comando de
excluir linha do Monaco. Os 218 testes Rust e 36 JavaScript passaram, assim como
fmt/clippy, sintaxe e build. Os 33 grupos da regressão completa também passaram,
incluindo hover pelo menu F1, correções rápidas, auto-imports e monitor incremental.
Evidência: `target/e2e-dialog-regression/report.json`.

Esse gate cobre X11/Xvfb e as interações descritas, sem substituir homologação
humana, IME/acentuação, acessibilidade, HiDPI ou Wayland nativo. Próximo marco:
publicar as entregas 11–12 e confirmar os dois gates no CI remoto; depois,
concluir a matriz manual de entrada e apresentação.

## Atualização — 2026-09-27

Correções rápidas e auto-imports foram publicados em `0479fbe`. O primeiro CI
falhou por desconexão do proxy WebDriver no teste de terminal; `e7fed1b` passou
a usar o transporte direto já validado localmente. O
[CI de `e7fed1b`](https://github.com/w3ti/Lyrnova/actions/runs/36313326018) passou,
incluindo as correções rápidas e os auto-imports com rust-analyzer real.

Entrega 11, local: monitor Linux por eventos, cache de até 100 mil entradas,
reconstrução das subárvores afetadas, ressincronização após overflow e modo de
compatibilidade visível. O Explorer reutiliza o cache e não relista por mudanças
apenas de conteúdo. Abas abertas mantêm verificação de conteúdo, rascunhos e
conflitos. A auditoria de metadados ocorre a cada 60 segundos; fechar a janela
ou trocar projeto libera as watches. Contrato e limites na
[ADR-0027](../architecture/ADR-0027-incremental-workspace-monitor.md).

Validação local: 218 testes Rust (206 unitários + 12 de fronteira), um opcional
ignorado, 36 testes JavaScript, fmt/clippy/sintaxe e build aprovados. Os 33 grupos
da jornada nativa passaram, incluindo o cenário de 6 mil arquivos e todos os
recursos Rust anteriores. Evidência: `target/e2e-incremental/report.json`.
O harness espera o foco do Monaco estabilizar antes de digitar; a entrada continua
via handlers DOM. Esta entrega 11 ainda não foi publicada nem executada no CI
remoto. Próximo item sugerido: homologação manual de teclado/mouse e diálogos.

## Atualização local — 2026-09-25

O HEAD publicado é `af79332`, contendo as entregas 1–9 abaixo. O
[CI remoto](https://github.com/w3ti/Lyrnova/actions/runs/36177855431) concluiu com
sucesso, incluindo a jornada nativa com rust-analyzer real. As indicações antigas
de CI pendente/falho e de entregas ainda sem commit estão superadas para esse HEAD.
A entrega 10, descrita a seguir, permanece local.

Décima entrega: correções rápidas Rust (Ctrl+.) e auto-imports no autocomplete.
As edições ficam em rascunhos, com validação de versão/conteúdo e desfazer; imports
são resolvidos antes de oferecer a sugestão para inserir símbolo e `use` juntos.
Resolução usa IDs opacos limitados à sessão; comandos e operações de arquivos não
são aceitos. Contrato: [ADR-0026](../architecture/ADR-0026-rust-quick-fixes-and-auto-imports.md).

Validação da entrega 10: 210 testes Rust (198 unitários + 12 de fronteira),
um opcional de provider ignorado, 35 testes JavaScript, fmt/clippy sem warnings,
sintaxe e build aprovados. Os 32 grupos da jornada nativa passaram com
rust-analyzer real: Ctrl+Espaço insere símbolo/import e um Ctrl+Z remove ambos;
Ctrl+. aplica a correção e permite desfazer, sem escrita implícita em disco.
Evidência: `target/e2e-quickfix/report.json`, via driver nativo direto e handlers
DOM. Homologação manual de entrada do SO permanece pendente. A entrega 10 ainda
não foi enviada ao CI remoto. Próximo item: monitor incremental para projetos maiores.

Primeira entrega após o levantamento do produto, ainda local:

- inspector com diff real de worktree/índice, seleção de removidos e renames;
- revisão integral antes de commit, token efêmero e árvore Git imutável;
- rejeição de revisão alterada, expirada, cancelada ou reutilizada;
- operações Git fora da thread da UI, limites de captura/tempo e paths literais;
- testes em repositórios temporários e smoke de UI no WebKitGTK;
- contrato e limites na ADR-0018.

Validação: build frontend/Rust, fmt, clippy sem warnings, 166 testes aprovados e
um teste opcional de provider ignorado; smoke da revisão no WebKitGTK aprovado.
O aplicativo foi iniciado na sessão gráfica.

Segunda entrega local: jornada nativa do MVP sem IA (#17), com o binário real,
perfil temporário, Git real e plugin externo de teste. Passaram edição/salvamento,
busca, revisão/commit, streaming, cancelamento de Tasks, falha de plugin, conflito
de salvamento, reinício sem repetir comandos e fechamento da janela. Editor e
base da interface usam 16 px por padrão, preservando preferências já salvas.

A jornada encontrou e corrigiu a ausência de permissões de escuta de eventos e
o vazamento de comandos filhos ao reiniciar/fechar o terminal. Emissão de eventos
pelo frontend permanece negada e é verificada no aplicativo real.

Validação da segunda entrega: 167 testes Rust aprovados, um teste opcional de provider ignorado,
fmt/clippy aprovados e 10 grupos de verificações nativas aprovados. O CI foi
ampliado, mas ainda não executado remotamente. Evidência e decisão de homologação
em [MVP nativo](../product/mvp-validation-2026-09-25.md); instruções e limites em
[tests/e2e](../../tests/e2e/README.md). A homologação manual de entrada e diálogos
continua pendente.

Terceira entrega local: terminal PTY Linux com Bash interativo e xterm.js,
ANSI/Unicode, resize, Ctrl+C/Ctrl+D, scrollback limitado e backpressure. Escrita,
resize, ACK e encerramento usam identificação de sessão; mensagens antigas não
atingem o terminal novo. O fechamento limpa grupos de jobs interativos, inclusive
background/disown na mesma sessão. Start/stop usam o pool bloqueante do Tauri.
Há uma sessão efêmera por workspace ativo, substituída ao reiniciar/trocar projeto.
Perfis, múltiplos terminais e plataformas não Linux continuam pendentes.

Contrato na [ADR-0019](../architecture/ADR-0019-linux-pty-terminal.md). A suíte Rust
agora tem 171 testes aprovados; os 12 grupos da jornada nativa passaram, incluindo
ANSI, resize, colagem, Ctrl+C/Ctrl+D, EOF, reinício sob saída contínua e rejeição
de sessões obsoletas. O smoke de revisão Git também passou (15 verificações).
Quarta entrega local: recuperação de sessão por projeto, com abas ordenadas,
documento ativo, cursor/rolagem e rascunhos. Snapshots privados, atômicos e limitados
preservam revisões antigas; arquivos alterados/excluídos exigem cópia ou descarte
explícito antes de reler. Há flush ao fechar/trocar projeto, erro visível de backup,
recusa de snapshot obsoleto e preservação de corrupção/schema desconhecido. O app
nativo também deixa de carregar as abas de demonstração. Contrato e limites na
[ADR-0020](../architecture/ADR-0020-editor-session-recovery.md).

Validação da quarta entrega: 176 testes Rust aprovados (164 unitários + 12 de
fronteira), um teste opcional de provider ignorado, fmt/clippy e build aprovados.
Os 16 grupos da jornada nativa passaram, incluindo abas/cursor/rolagem, arquivos
alterados/excluídos, cópia, descarte, flush no fechamento, sessão vazia e schema
incompatível preservado. Evidência local em `target/e2e-session/report.json`.
Entrada e prompts seguem pelo harness DOM; seletores nativos/troca de projeto
exigem homologação manual. A repetição do smoke visual Git nesta fase ficou
pendente: a sessão do host estava bloqueada (`LockedHint=yes`) e o WebView GTK
não recebeu nenhum frame de animação. O fluxo Git passou na jornada nativa.
O binário atualizado foi iniciado, preservando a janela anterior. Nenhuma
alteração foi publicada.

Quinta entrega local: diagnósticos LSP iniciais de Rust nos documentos abertos,
com marcadores no Monaco e painel Problemas navegável. A ativação exige revisão
das permissões do plugin e um rust-analyzer standalone no PATH. O servidor roda
isolado, sem rede e com workspace somente leitura; build scripts e proc macros
ficam desabilitados. Versões e sessões impedem diagnósticos obsoletos; falhas
permitem reinício e desativar o plugin encerra o processo. Dependências externas,
biblioteca padrão e demais recursos de linguagem ainda não estão integrados.
Contrato na [ADR-0021](../architecture/ADR-0021-rust-lsp-diagnostics.md) e uso no
[guia Rust](../plugins/rust-diagnostics.md).

Validação: 186 testes Rust aprovados (174 unitários + 12 de fronteira), um teste
opcional ignorado, clippy sem warnings e 20 grupos da jornada nativa aprovados
com rust-analyzer real, incluindo crash/reinício e revogação de permissões.
Evidência local: `target/e2e-lsp/report.json`. CI remoto e homologação manual
continuam pendentes. Os ícones da barra esquerda passam a 24 px no tamanho
padrão da interface, com botões de 48 px (42 px na densidade compacta).

As cinco entregas acima foram publicadas em `7f3413a` em `main`. O CI remoto
passou nas verificações de Rust/frontend e falhou na espera dos diagnósticos
nativos. O runner usava rustup no HOME, fora do sandbox de análise. O ajuste local
instala Cargo/rustc da distribuição no CI e informa a ausência dessas ferramentas
na UI; a confirmação remota desse ajuste ainda depende de nova execução.

Sexta entrega local: definição (F12) e hover Rust, com abertura de outra aba,
consultas limitadas/canceláveis, descarte de resultados obsoletos e documentação
inerte. Contrato e limites na [ADR-0022](../architecture/ADR-0022-rust-symbol-queries.md).
Validação da sexta entrega: 192 testes Rust aprovados (180 unitários + 12 de
fronteira), um opcional de provider ignorado, seis testes de providers JavaScript,
fmt/clippy e build aprovados. Os 21 grupos da jornada nativa passaram, incluindo
hover literal e F12 abrindo uma definição em outra aba com rust-analyzer real.
Evidência local: `target/e2e-symbols/report.json`. Entrada continua via handlers
DOM; teclado/mouse do SO e acessibilidade não foram homologados por esta suíte.
Sétima entrega local: seleção explícita de toolchains instaladas (sistema/rustup),
fontes da biblioteca padrão e resolução offline de dependências. A revisão mostra
os diretórios e exige opção explícita para compartilhar registry; configuração e
credenciais pessoais ficam fora do sandbox. A autorização é por workspace/sessão,
com token limitado, revalidação e revogação no lifecycle. F12 externo abre um
visualizador somente leitura; erros de metadados aparecem no painel. A resolução
continua sem build scripts, proc macros, downloads ou alterações em Cargo.lock.
Contrato na [ADR-0023](../architecture/ADR-0023-rust-toolchains-and-local-dependencies.md).

Validação da sétima entrega: 196 testes Rust (184 unitários + 12 de fronteira),
um opcional ignorado, dez testes JavaScript e 23 grupos nativos aprovados, incluindo
crate em cache, toolchain instalada e definição de Option na biblioteca padrão.
Relatório local: `target/e2e-environment/report.json`. As fontes Rust 1.97.0 usadas
no teste foram obtidas e verificadas em diretório temporário, sem instalar pacotes
no host. O CI foi ampliado com rust-src, mas não houve nova execução remota.
Oitava entrega local: os quatro recursos de edição Rust foram implementados.
Autocomplete semântico (Ctrl+Espaço), referências navegáveis (Shift+F12), renomeação
entre arquivos (F2) e formatação (Shift+Alt+F). Rename abre arquivos fechados como
rascunhos versionados, preserva conteúdo não salvo e conflitos, participa do backup
e permite desfazer por arquivo. Rustfmt usa a toolchain selecionada no sandbox.
Não há salvamento automático, auto-import nem operações de arquivos pelo LSP.
Contrato e limites na [ADR-0024](../architecture/ADR-0024-rust-editing-actions.md).

Validação da oitava entrega: 201 testes Rust (189 unitários + 12 de fronteira),
um opcional ignorado, 20 testes JavaScript e 27 grupos nativos aprovados. Os quatro
recursos foram exercitados com servidor/ferramenta reais e atalhos no Monaco,
incluindo aceitação de completion, referências, arquivo fechado na renomeação,
ausência de escrita no disco e desfazer. Build, fmt, clippy e sintaxe aprovados.
Relatório: `target/e2e-rust-actions/report.json`. A confirmação de sincronização
usa versões dos documentos independentemente da publicação de diagnósticos.
O atalho Shift+Alt+F foi explicitamente associado à formatação no Linux.
Alterações das entregas seis a oito permanecem locais, sem novo commit/push;
o gate remoto ainda precisa de nova execução após publicação autorizada.
Nona entrega local: detecção de alterações externas no workspace, com polling
limitado a cada 1,5 segundo, hashes de abas/fontes Rust e leituras sem seguir
symlinks. Abas limpas recarregam; rascunhos, arquivos removidos/binários e conflitos
são preservados. Explorer/busca atualizam sem perder pastas recolhidas. Salvamento,
leitura e troca de projeto têm proteção contra respostas obsoletas. Mudanças Rust
invalidam consultas no backend e reiniciam a análise com rascunhos e ambiente
preservados. Contrato/limites na [ADR-0025](../architecture/ADR-0025-workspace-external-changes.md).

Validação: 206 testes Rust (194 unitários + 12 de fronteira), um opcional ignorado,
32 testes JavaScript e 30 grupos nativos aprovados. Build, fmt, clippy e sintaxe
aprovados. O gate verificou alterações reais no disco, abas ativas/inativas,
conflito antes de salvar, reload explícito, recriação, substituição binária, save
próprio sem conflito e hover/definição após alterar uma fonte Rust fechada.
Evidência: `target/e2e-watch/report.json`. O proxy local tauri-driver encerrou
conexões em tentativas anteriores; o gate completo passou com `--direct-native`,
registrado no relatório, mantendo binário/WebView/IPC reais e criação das sessões
pelo tauri-driver. Nenhum comando de mutação é repetido implicitamente.
Não houve novo commit/push; o CI remoto destas alterações continua pendente.
Próximas frentes possíveis: code actions/auto-imports e homologação manual dos
fluxos de edição. Monitor incremental para projetos grandes e diretórios externos
segue pendente.

## Registro histórico de 2026-09-02

Data: 2026-09-02
Commit-base: `5ceddb8`
Estado: item #15 publicado; item #16 em implementação local.

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
- Approvals do agente e revisões de Tasks agora devolvem SHA-256 da ação tipada; o
  frontend precisa repetir esse hash junto ao token, e alterações de comando, cwd,
  arquivos, domínio, environment ou política falham fechadas.
- Aprovações expiram em cinco minutos, são consumidas uma única vez e pendências são
  negadas no encerramento da sessão, troca de workspace ou logout.
- “Permitir na sessão” é uma regra local limitada à conversa e ao hash exato. O
  provider recebe apenas aceite pontual; regras são visíveis e revogáveis nas
  Configurações e sugestões externas de ampliação são ignoradas.
- A interface diferencia risco elevado e crítico, usa ação visual destrutiva para
  comandos perigosos, destaca dados redigidos e mostra validade, escopo e hash.
- O histórico seguro em memória registra categoria, decisão, origem, horário e hash,
  sem comandos, diffs ou valores de environment. A ADR-0017 documenta o contrato.

## Validações já executadas em 2026-09-02

- `npm run check --prefix ui`
- `npm run build --prefix ui`
- `cargo fmt --all -- --check`
- `cargo check --workspace --offline`
- `cargo clippy --workspace --all-targets --offline -- -D warnings`
- `cargo test --workspace` (152 testes aprovados e 1 teste de integração
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
3. concluir a validação e revisão do item #16 antes de selecionar o próximo issue.

## Estado do repositório

O commit `5ceddb8` contém o item #15 completo, incluindo Tasks, grants e revisão na
interface. O item #16 está no worktree, ainda sem commit. Nenhum pacote OBS ou
release foi realizado.
