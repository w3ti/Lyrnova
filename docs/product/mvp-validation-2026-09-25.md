# Validação nativa do MVP — 2026-09-25

Atualização posterior nesta data: o PTY Linux foi implementado e a jornada foi
ampliada para ANSI, resize, Ctrl+C/Ctrl+D e reinício após EOF. O backend passou em
171 testes Rust, incluindo limites e limpeza de jobs foreground/background/disown.
Os 12 grupos da jornada ampliada passaram, incluindo reinício sob saída contínua
e recusa de escrita/encerramento com sessão obsoleta; o smoke visual Git passou
em 15 verificações. A entrada continua via handlers DOM na janela real.
Veja a [ADR-0019](../architecture/ADR-0019-linux-pty-terminal.md).
O registro abaixo descreve a execução anterior ao PTY; suas medições permanecem
históricas. A evidência local da jornada ampliada fica em `target/e2e-pty`.

Nova atualização local: recuperação do editor implementada conforme a
[ADR-0020](../architecture/ADR-0020-editor-session-recovery.md). Passaram 176 testes
Rust (164 unitários + 12 de fronteira) e os 16 grupos da jornada nativa: restauração
de abas, arquivo ativo, cursor e rolagem; rascunhos com disco alterado/excluído;
cópia/descarte explícito; flush ao fechar; sessão vazia; tokens obsoletos e schema
incompatível. Relatório: `target/e2e-session/report.json`. A execução registrou
2,578 s de inicialização, 0,008 s para salvar e soma de RSS de 1.176.340 KiB em seis
processos, com as mesmas ressalvas de build debug e páginas compartilhadas abaixo.
As respostas aos prompts são injetadas no harness DOM; não homologam diálogos do SO.
A repetição do smoke visual Git nesta fase não foi concluída: sessão gráfica
bloqueada (`LockedHint=yes`) e nenhum callback de animação no WebView GTK. O fluxo
Git da jornada nativa passou; o smoke separado precisa de tela renderizando.
O registro restante é histórico e não representa pendências já cobertas nesta fase.

Atualização LSP: 186 testes Rust aprovados (174 unitários + 12 de fronteira), um
teste opcional ignorado e clippy aprovado. Os 20 grupos da jornada nativa passaram
com rust-analyzer standalone 0.3.3057: revisão das permissões, diagnósticos de
rascunho/UTF-16, correção, isolamento sem escrita/build scripts, versões e sessões
obsoletas, crash/reinício e desativação encerrando o servidor. Relatório local:
`target/e2e-lsp/report.json`. O servidor foi obtido em diretório temporário; não
houve instalação de pacote no sistema. As medições de inicialização e RSS deste
relatório antecedem a fase LSP e não medem o consumo do servidor de linguagem.
O CI inclui o binário fixado com checksum, mas não foi executado remotamente.

Atualização definição/hover: 192 testes Rust e seis testes dos providers
JavaScript aprovados; fmt/clippy e build aprovados. Os 21 grupos nativos passaram
com navegação F12 para arquivo ainda não aberto, cursor no símbolo, hover pelo
Monaco e documentação sem links/imagens ativos. Evidência local em
`target/e2e-symbols/report.json`; permanece o limite de entrada por handlers DOM.

O [CI de `7f3413a`](https://github.com/w3ti/Lyrnova/actions/runs/36166921364) passou
nas etapas Rust/frontend e falhou esperando diagnósticos na jornada nativa. O
workflow instala Rust via rustup no HOME, que não é visível no sandbox. A correção
local adiciona Cargo/rustc da distribuição no CI e um erro explícito na aplicação
quando essas ferramentas faltam. A confirmação remota depende de nova execução.

Atualização ambientes Rust: 196 testes Rust (184 + 12), um opcional ignorado,
dez testes JavaScript e 23 grupos nativos aprovados. A jornada usa cópias reais
Cargo/rustc em uma instalação temporária, cache de crate isolado e rust-src 1.97.0
com checksum oficial verificado. Cobertura: cancelar/aplicar revisão, seleção da
toolchain, definição/hover de dependência, visualizador somente leitura, resolução
de Option da biblioteca padrão e preservação do lockfile. Evidência:
`target/e2e-environment/report.json`. A amostra de memória antecede essa fase;
não mede o consumo do servidor com stdlib. Toolchains arbitrárias e projetos com
build scripts/proc macros ainda não estão homologados. CI atualizado, sem execução
remota destas alterações.

**Decisão:** gate automatizado local aprovado para o escopo abaixo. A homologação
integral do MVP (#17) continua pendente de entrada/diálogos nativos, execução do
CI e matriz de ambientes. Nenhuma issue foi fechada e nenhuma release publicada.

Atualização ações Rust: 201 testes Rust (189 + 12), um opcional ignorado,
20 testes JavaScript e 27 grupos nativos aprovados. Ctrl+Espaço aceita completion
semântico; Shift+F12 navega referências; F2 altera chamada e definição em arquivo
antes fechado; Shift+Alt+F aplica rustfmt. A suíte confirma rascunhos sem gravação
em disco e desfazer por arquivo. Testes unitários cobrem cancelamento, preparação
integral, conflitos, ranges UTF-16, operações externas e resposta obsoleta.
Build/fmt/clippy/sintaxe aprovados. Evidência: `target/e2e-rust-actions/report.json`.
A jornada continua usando handlers DOM no WebView nativo, sem homologação de entrada
do SO. CI remoto das mudanças locais continua pendente.

Atualização mudanças externas: 206 testes Rust (194 + 12), um opcional ignorado,
32 testes JavaScript e 30 grupos nativos aprovados; build/fmt/clippy/sintaxe aprovados.
O gate usa alterações reais em disco: criação, rename, remoção, recarga de abas
limpas ativas/inativas, conflito antes de salvar, preservação de rascunhos, binário,
recriação e save próprio. Alterar uma fonte Rust fechada reinicia a análise,
atualiza hover/definição e preserva o rascunho do chamador. Evidência local:
`target/e2e-watch/report.json`. Após perdas de conexão no proxy local, a execução
completa passou com `--direct-native`; sessões seguem criadas pelo tauri-driver e
os comandos chegam ao mesmo WebView real. O relatório identifica esse transporte.
CI remoto e homologação de entrada do SO permanecem pendentes.

## Ambiente e resultado

- Checkout baseado em `c338103`, com as entregas locais de diff/revisão e esta suíte.
- Linux x86_64, kernel `6.12.0-160100.5-default`, glibc 2.40, build debug.
- Wry 0.55.1, WebKitGTK 2.52.5, tauri-driver 2.0.5; sessão via XWayland.
- Driver local WebKitWebDriver 2.50.6 extraído em `/tmp`, sem instalar pacotes no
  sistema. O CI usa o driver da mesma distribuição que fornece o WebKitGTK.
- 167 testes Rust aprovados (155 unitários + 12 de fronteira), um teste opcional
  de provider real ignorado; fmt e clippy sem warnings.
- Dez grupos de verificações na jornada nativa aprovados, com IPC de produção.

Uma execução final registrou 2,441 s desde a abertura da sessão até o primeiro
documento renderizado, 8 ms para salvar e 1.126.492 KiB de RSS somado em seis
processos (driver e descendentes). Isso inclui páginas compartilhadas contadas
repetidamente: não representa o consumo exclusivo do IDE, pico ou benchmark de
release. Não há gate de desempenho definido; projetos grandes ainda precisam
de medição própria. O relatório bruto local fica em `target/e2e/report.json`.

## Jornadas verificadas

| Fluxo | Evidência |
|---|---|
| Perfil limpo sem IA | Projeto recente restaurado, controles de IA ocultos, editor/base do IDE em 16 px. |
| Permissões da janela | Consulta de maximização permitida; emissão de eventos pelo frontend recusada. |
| Editor e Explorer | Edição no Monaco sem escrita implícita; salvar muda o arquivo; busca encontra o conteúdo. |
| Git | Diff real, stage, revisão cancelada sem commit, confirmação cria o commit com conteúdo esperado. |
| Terminal | stdout/stderr chegam à tela; reiniciar encerra comando filho em background. |
| Plugin e Tasks | Ativação de pacote externo, revisão sem efeito prévio, Bubblewrap forte, streaming e cancelamento de descendente. |
| Falha de plugin | Runtime encerra durante consulta; erro aparece e editor segue funcional. |
| Concorrência de arquivo | Edição externa recusa salvamento obsoleto, preserva arquivo e rascunho. |
| Reinício | Projeto/conteúdo em disco continuam acessíveis; comando anterior não é repetido. |
| Fechamento normal | Fechar a janela termina os comandos filhos do terminal. |

O teste encontrou duas falhas de produto: listeners de streaming estavam negados
pelo ACL da janela e o terminal encerrava apenas o shell. Foram concedidos apenas
listen/unlisten e a consulta de maximização já usada pela interface; emissão
continua negada. O shell agora tem grupo próprio, encerrado ao descartar a sessão
ou destruir a janela. Há teste Rust de regressão para o comando filho.

## Aceites restantes

- O driver deste ambiente recusa entrada nativa. A suíte aciona handlers DOM no
  WebView real; teclado/mouse do SO, IME e acessibilidade exigem homologação própria.
- Seletores nativos de criar/abrir projeto e instalar pacote precisam de jornada
  manual. O teste prepara histórico e pacote usando os contratos de produção.
- Reinício não significa recuperação de abas, cursor ou rascunhos; isso permanece
  uma entrega funcional separada.
- PTY, crash abrupto, processos que abandonam deliberadamente o grupo, Wayland
  nativo, outras plataformas, grandes projetos e desempenho de release não foram
  aprovados por esta execução.
- O workflow está preparado para Ubuntu/Xvfb e upload de evidência; sua execução
  remota depende de publicação autorizada das alterações.

Reprodução e limites completos: [guia da suíte](../../tests/e2e/README.md).
